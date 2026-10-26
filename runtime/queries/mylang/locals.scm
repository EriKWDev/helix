; Scopes
(block) @local.scope
(proc_decl) @local.scope
(gen_decl) @local.scope

; Definitions
(param (identifier) @local.definition)
(declaration (identifier) @local.definition)

; References
(identifier) @local.reference
