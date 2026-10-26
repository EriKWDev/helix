; =============================================================================
; mylang highlights.scm
; =============================================================================

; ─── Catch-all baselines for all identifier alias node types ─────────────────
(identifier)          @variable
(type_identifier)     @type
(field_identifier)    @variable.other.member
(method_identifier)   @function.method
(function_identifier) @function

; ─── Non-identifier node types ───────────────────────────────────────────────
(wildcard_pattern)  @variable.builtin
(backtick_name)     @operator
(unit_type)         @type.builtin
(infer_type)        @type.builtin

; ─── Types in type-position ──────────────────────────────────────────────────
(named_type (type_identifier) @type.builtin
  (#any-of? @type.builtin
    "i8" "i16" "i32" "i64" "u8" "u16" "u32" "u64" "u128"
    "f32" "f64" "f128" "int" "uint" "usize" "isize"
    "bool" "char" "str" "void"))

; Generic parameters  $T  $N
(generic_param_type (identifier) @type.parameter)
(generic_params     (identifier) @type.parameter)

; ─── Tags  @(name)  @!(name) ─────────────────────────────────────────────────
(tag "(" . (identifier) @attribute)

; ─── Patterns ────────────────────────────────────────────────────────────────
(bind_pattern    (identifier) @variable)
(bind_as_pattern (identifier) @variable)

; ─── Enum variants ───────────────────────────────────────────────────────────
(enum_variant         (identifier) @type.enum.variant)
(enum_variant_expr    (identifier) @type.enum.variant)
(enum_variant_pattern (identifier) @type.enum.variant)

; ─── Parameters ──────────────────────────────────────────────────────────────
(param (identifier) @variable.parameter)

; ─── Declaration names ───────────────────────────────────────────────────────
; PascalCase declarations are type-like (aliases, newtypes, etc.).
(declaration (identifier) @type
  (#match? @type "^[A-Z]"))

; Let-bindings and typed locals
(declaration (identifier) @variable ":=")
(declaration (identifier) @variable ":")

; ALL_CAPS = compile-time constant
(declaration (identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

; Procedure / generator declarations use function_identifier on the LHS
(declaration (function_identifier) @function (gen_decl))
(declaration (function_identifier) @function (proc_decl))
(declaration (backtick_name) @operator (proc_decl))

; ─── Static access  Container::member ────────────────────────────────────────
(static_access_expr container: (identifier) @type)
(static_access_expr member:    (identifier) @function)

(call_expr
  (static_access_expr member: (identifier) @function.call))

; ─── Calls ───────────────────────────────────────────────────────────────────
; Method calls  obj.method()  — method_identifier is the callee node type
(call_expr (field_expr (field_identifier) @function.method))

; Regular calls — leading anchor matches only the callee identifier
(call_expr . (identifier) @function.call)

; ─── Keywords ────────────────────────────────────────────────────────────────
["proc" "gen" "operator"]                        @keyword.function
["struct" "enum" "bit_field" "module" "context"] @keyword.storage.type
["import"]                                       @keyword.control.import
["return" "yield"]                               @keyword.control.return
["if" "else" "match"]                            @keyword.control.conditional
["for" "while" "in" "break" "continue"]          @keyword.control.repeat
["defer"]                                        @keyword.control.exception
["as"]                                           @keyword.operator
["mut"]                                          @keyword.storage.modifier

; ─── Operators ───────────────────────────────────────────────────────────────
["+" "-" "*" "/" "%" "==" "!=" "<" "<=" ">" ">="
 "&&" "||" "!" "&" "|" "^" "<<" ">>" "++" "--"] @operator
["=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="] @operator
["->" "=>" "::" ":=" ":" ".." "..=" "..."] @operator

; ─── Punctuation ─────────────────────────────────────────────────────────────
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";"]                  @punctuation.delimiter
["."]                      @punctuation.delimiter

; ─── Literals ────────────────────────────────────────────────────────────────
(int_literal)       @number
(float_literal)     @number.float
(string_literal)    @string
(bool_literal)      @constant.builtin
(nil_literal)       @constant.builtin
(undefined_literal) @constant.builtin
(zeroed_literal)    @constant.builtin

; ─── Comments ────────────────────────────────────────────────────────────────
(line_comment)  @comment
(block_comment) @comment

((line_comment) @comment.note    (#match? @comment.note    "\\bNOTE\\b"))
((line_comment) @comment.todo    (#match? @comment.todo    "\\bTODO\\b"))
((line_comment) @comment.warning (#match? @comment.warning "\\bHACK\\b"))
((line_comment) @comment.error   (#match? @comment.error   "\\bFIXME\\b"))
((block_comment) @comment.note    (#match? @comment.note    "\\bNOTE\\b"))
((block_comment) @comment.todo    (#match? @comment.todo    "\\bTODO\\b"))
((block_comment) @comment.warning (#match? @comment.warning "\\bHACK\\b"))
((block_comment) @comment.error   (#match? @comment.error   "\\bFIXME\\b"))
