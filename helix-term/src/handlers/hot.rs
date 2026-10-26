//! Opt-in, bounded unsaved-buffer bridge. Only the sender thread touches the socket.
#[cfg(unix)]
mod unix {
    use crate::events::PostCommand;
    use arc_swap::ArcSwapOption;
    use helix_core::{ChangeSet, Operation, Rope};
    use helix_event::register_hook;
    use helix_view::{
        events::{DocumentDidChange, DocumentDidClose, DocumentDidOpen, DocumentDidReload},
        Document, DocumentId,
    };
    use std::{
        collections::HashMap,
        io::{self, Write},
        net::Shutdown,
        os::unix::net::UnixStream,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    struct Snapshot {
        version: u64,
        text: Option<Rope>,
        delta: Option<(u64, Rope, ChangeSet)>,
    }
    struct Slot {
        path: PathBuf,
        latest: ArcSwapOption<Snapshot>,
    }
    struct Controller {
        active: ArcSwapOption<Publisher>,
    }
    thread_local! {
        static CONTROLLER: std::cell::RefCell<Option<Arc<Controller>>> = const { std::cell::RefCell::new(None) };
    }
    struct Publisher {
        socket: PathBuf,
        cancelled: Arc<AtomicBool>,
        connection: Arc<ArcSwapOption<UnixStream>>,
        // Accessed only by synchronous editor hooks; the sender never takes this lock.
        documents: Mutex<HashMap<DocumentId, Arc<Slot>>>,
        register: mpsc::Sender<Arc<Slot>>,
        wake: mpsc::SyncSender<()>,
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            self.cancelled.store(true, Ordering::Release);
            if let Some(stream) = self.connection.load_full() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            let _ = self.wake.try_send(());
        }
    }
    impl Publisher {
        fn publish(&self, doc: &Document, delta: Option<(Rope, ChangeSet)>) {
            let Some(path) = doc.path() else { return };
            if path.extension().is_none_or(|ext| ext != "ml") {
                return;
            }
            let mut documents = self.documents.lock().unwrap();
            if documents
                .get(&doc.id())
                .is_some_and(|slot| slot.path != path)
            {
                let old = documents.remove(&doc.id()).unwrap();
                old.latest.store(Some(Arc::new(Snapshot {
                    version: 0,
                    text: None,
                    delta: None,
                })));
            }
            let slot = documents.entry(doc.id()).or_insert_with(|| {
                let slot = Arc::new(Slot {
                    path: path.to_owned(),
                    latest: ArcSwapOption::empty(),
                });
                let _ = self.register.send(slot.clone());
                slot
            });
            let previous = slot.latest.load_full();
            let delta = delta.and_then(|(old, changes)| {
                previous
                    .as_ref()
                    .filter(|p| p.version.checked_add(1) == Some(doc.version() as u64))
                    .map(|p| (p.version, old, changes))
            });
            slot.latest.store(Some(Arc::new(Snapshot {
                version: doc.version() as u64,
                text: Some(doc.text().clone()),
                delta,
            })));
            let _ = self.wake.try_send(());
        }
        fn close(&self, doc: &Document) {
            if let Some(slot) = self.documents.lock().unwrap().remove(&doc.id()) {
                slot.latest.store(Some(Arc::new(Snapshot {
                    version: 0,
                    text: None,
                    delta: None,
                })));
                let _ = self.wake.try_send(());
            }
        }
    }

    fn bytes(out: &mut Vec<u8>, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
    }
    fn encode(slot: &Slot, snapshot: &Snapshot, sent: Option<u64>) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        let mut out = Vec::new();
        let delta = snapshot
            .delta
            .as_ref()
            .filter(|(base, _, _)| Some(*base) == sent);
        out.push(if snapshot.text.is_none() {
            3
        } else if delta.is_some() {
            2
        } else {
            1
        });
        bytes(&mut out, slot.path.as_os_str().as_bytes());
        if let Some(text) = &snapshot.text {
            if let Some((base, old, changes)) = delta {
                out.extend_from_slice(&base.to_le_bytes());
                out.extend_from_slice(&snapshot.version.to_le_bytes());
                let edits: Vec<_> = changes.changes_iter().collect();
                out.extend_from_slice(&(edits.len() as u32).to_le_bytes());
                for (start, end, inserted) in edits {
                    out.extend_from_slice(&(old.char_to_byte(start) as u64).to_le_bytes());
                    out.extend_from_slice(&(old.char_to_byte(end) as u64).to_le_bytes());
                    bytes(&mut out, inserted.as_ref().map_or(b"", |s| s.as_bytes()));
                }
            } else {
                out.extend_from_slice(&snapshot.version.to_le_bytes());
                out.extend_from_slice(&(text.len_bytes() as u32).to_le_bytes());
                for chunk in text.chunks() {
                    out.extend_from_slice(chunk.as_bytes());
                }
            }
        }
        out
    }
    fn sender(
        path: PathBuf,
        register: mpsc::Receiver<Arc<Slot>>,
        wake: mpsc::Receiver<()>,
        cancelled: Arc<AtomicBool>,
        connection: Arc<ArcSwapOption<UnixStream>>,
    ) {
        let mut slots: Vec<(Arc<Slot>, Option<u64>)> = Vec::new();
        let mut stream: Option<UnixStream> = None;
        loop {
            let wait = wake.recv_timeout(Duration::from_millis(250));
            if matches!(wait, Err(mpsc::RecvTimeoutError::Disconnected)) {
                return;
            }
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            slots.extend(register.try_iter().map(|slot| (slot, None)));
            if stream.is_none() {
                slots.retain(|(slot, _)| {
                    slot.latest.load().as_ref().is_none_or(|s| s.text.is_some())
                });
                match UnixStream::connect(&path) {
                    Ok(mut connected) => {
                        let _ = connected.set_write_timeout(Some(Duration::from_secs(1)));
                        if connected.write_all(b"HXHOT001").is_err() {
                            continue;
                        }
                        connection.store(connected.try_clone().ok().map(Arc::new));
                        if cancelled.load(Ordering::Acquire) {
                            let _ = connected.shutdown(Shutdown::Both);
                            connection.store(None);
                            return;
                        }
                        stream = Some(connected);
                        for (_, sent) in &mut slots {
                            *sent = None;
                        }
                    }
                    Err(_) => continue,
                }
            }
            for (slot, sent) in &mut slots {
                let Some(snapshot) = slot.latest.load_full() else {
                    continue;
                };
                if snapshot.text.is_some() && *sent == Some(snapshot.version) {
                    continue;
                }
                let packet = encode(slot, &snapshot, *sent);
                let result = (|| -> io::Result<()> {
                    let stream = stream.as_mut().unwrap();
                    stream.write_all(&(packet.len() as u32).to_le_bytes())?;
                    stream.write_all(&packet)
                })();
                if let Err(error) = result {
                    log::warn!("hot compiler disconnected: {error}");
                    stream = None;
                    connection.store(None);
                    break;
                }
                *sent = Some(snapshot.version);
            }
            slots.retain(|(slot, _)| slot.latest.load().as_ref().is_none_or(|s| s.text.is_some()));
        }
    }
    fn start(path: PathBuf) -> anyhow::Result<Arc<Publisher>> {
        let (register_tx, register_rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let connection = Arc::new(ArcSwapOption::empty());
        let publisher = Arc::new(Publisher {
            socket: path.clone(),
            cancelled: cancelled.clone(),
            connection: connection.clone(),
            documents: Mutex::new(HashMap::new()),
            register: register_tx,
            wake: wake_tx,
        });
        thread::Builder::new()
            .name("hot-source-sender".into())
            .spawn(move || sender(path, register_rx, wake_rx, cancelled, connection))?;
        Ok(publisher)
    }
    pub fn register_hooks() {
        let controller = Arc::new(Controller {
            active: ArcSwapOption::empty(),
        });
        CONTROLLER.with(|current| *current.borrow_mut() = Some(controller.clone()));
        let open = controller.clone();
        register_hook!(move |event: &mut DocumentDidOpen<'_>| {
            if let (Some(publisher), Some(doc)) =
                (open.active.load_full(), event.editor.document(event.doc))
            {
                if doc.is_modified() {
                    publisher.publish(doc, None);
                }
            }
            Ok(())
        });
        let reloaded = controller.clone();
        register_hook!(move |event: &mut DocumentDidReload<'_>| {
            if let Some(publisher) = reloaded.active.load_full() {
                publisher.close(event.doc);
            }
            Ok(())
        });
        let clean = controller.clone();
        register_hook!(move |event: &mut PostCommand<'_, '_>| {
            if let Some(publisher) = clean.active.load_full() {
                let doc = doc!(event.cx.editor);
                if !doc.is_modified() {
                    publisher.close(doc);
                }
            }
            Ok(())
        });
        let change = controller.clone();
        register_hook!(move |event: &mut DocumentDidChange<'_>| {
            if !event.ghost_transaction {
                if let Some(publisher) = change.active.load_full() {
                    // Bound copying of SmartString insertions in the editor thread.
                    let mut inserted = 0;
                    let small = event.changes.changes().len() <= 256
                        && event.changes.changes().iter().all(|op| {
                            if let Operation::Insert(text) = op {
                                inserted += text.len();
                            }
                            inserted <= 64 * 1024
                        });
                    let delta = small.then(|| (event.old_text.clone(), event.changes.clone()));
                    publisher.publish(event.doc, delta);
                }
            }
            Ok(())
        });
        register_hook!(move |event: &mut DocumentDidClose<'_>| {
            if let Some(publisher) = controller.active.load_full() {
                publisher.close(&event.doc);
            }
            Ok(())
        });
    }
    pub fn saved(doc: &Document) {
        CONTROLLER.with(|current| {
            if let Some(controller) = current.borrow().as_ref() {
                if let Some(publisher) = controller.active.load_full() {
                    if doc.is_modified() {
                        publisher.publish(doc, None);
                    } else {
                        publisher.close(doc);
                    }
                }
            }
        });
    }
    pub fn command(argument: Option<&str>, editor: &helix_view::Editor) -> anyhow::Result<String> {
        CONTROLLER.with(|current| {
            let current = current.borrow();
            let controller = current
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("hot hooks not initialized"))?;
            let status = || match controller.active.load_full() {
                Some(publisher) => format!(
                    "hot: {} {}",
                    if publisher.connection.load().is_some() {
                        "connected to"
                    } else {
                        "connecting to"
                    },
                    publisher.socket.display()
                ),
                None => "hot: off".to_owned(),
            };
            if argument == Some("status") {
                return Ok(status());
            }
            if argument == Some("off") || (argument.is_none() && controller.active.load().is_some())
            {
                controller.active.store(None);
                return Ok("hot: off".to_owned());
            }
            let path = helix_stdx::path::canonicalize(argument.unwrap_or("out/hot-edit.sock"));
            let publisher = start(path)?;
            // Stop the previous session before publishing any buffers to the new one.
            controller.active.store(None);
            for doc in editor.documents().filter(|doc| doc.is_modified()) {
                publisher.publish(doc, None);
            }
            controller.active.store(Some(publisher));
            Ok(status())
        })
    }
}

pub fn register_hooks() {
    #[cfg(unix)]
    unix::register_hooks();
}

pub fn command(argument: Option<&str>, editor: &helix_view::Editor) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        unix::command(argument, editor)
    }
    #[cfg(not(unix))]
    {
        let _ = (argument, editor);
        anyhow::bail!("hot mode currently requires Unix-domain sockets")
    }
}

pub fn saved(doc: &helix_view::Document) {
    #[cfg(unix)]
    unix::saved(doc);
    #[cfg(not(unix))]
    let _ = doc;
}
