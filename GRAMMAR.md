# BYO/OS Command Grammar

PEG grammar for the BYO/OS command language. The scanner extracts the
payload between `ESC _ B` and `ESC \`; this grammar defines the
structure within that payload.

```peg
# -- Top level --

batch      = whitespace? (command whitespace?)*
command    = upsert | destroy | patch | event | request | response

# -- Object operations --

upsert     = '+' type id prop* children?
destroy    = '-' type id
patch      = '@' type id prop* children?
children   = '{' whitespace? (command whitespace?)* '}'

# -- Events --

event      = '!' (ack | other_event)
ack        = 'ack' type seqnum prop*
other_event = type seqnum id prop*

# -- Requests --

request      = '?' (claim_req | unclaim_req | observe_req | unobserve_req | other_req)
claim_req    = 'claim' seqnum types
unclaim_req  = 'unclaim' seqnum types
observe_req  = 'observe' seqnum types
unobserve_req = 'unobserve' seqnum types
types        = type (',' type)*
other_req    = type seqnum id prop*

# -- Responses --

response   = '.' (expand_res | other_res)
expand_res = 'expand' seqnum children
other_res  = type seqnum prop* children?

# -- Properties --

prop       = '~' name | name '=' value | name
value      = dqstring | sqstring | bare

# -- Strings --

dqstring   = '"' dqchar* '"'
dqchar     = [^"\\] | escape
sqstring   = "'" sqchar* "'"
sqchar     = [^'\\] | escape
escape     = '\\' ["'\\/nrt0]
bare       = [^\s{}="'~\\,+\-@!?.] [^\s{}="'~\\,]*

# -- Atoms --

type       = [a-zA-Z][a-zA-Z0-9._-]*
id         = [a-zA-Z_][a-zA-Z0-9_:-]*
name       = [a-zA-Z][a-zA-Z0-9_-]*
seqnum     = [0-9]+
whitespace = [ \t\n\r]+
```

## Escape sequences

Supported in quoted strings only. Bare values do not support escaping
— a backslash in a bare value is a parse error.

| Escape | Value                   |
|--------|-------------------------|
| `\"`   | literal `"`             |
| `\'`   | literal `'`             |
| `\\`   | literal `\`             |
| `\/`   | literal `/`             |
| `\n`   | newline (U+000A)        |
| `\r`   | carriage return (U+000D)|
| `\t`   | tab (U+0009)            |
| `\0`   | null (U+0000)           |

Unrecognized escape sequences (e.g. `\q`) are parse errors.

## Structural notes

- **Recursive children.** `children` is recursive in the grammar,
  enforcing balanced `{`/`}`. The parser emits flat `Push`/`Pop`
  commands in the output stream.

- **Children targets.** `upsert` (`+`), `patch` (`@`), and
  `response` (`.`) can have children blocks.

- **Greedy props.** `prop*` is delimited by whitespace. The parser
  greedily consumes props until it hits a character that starts a
  new command (`+`, `-`, `@`, `!`, `?`, `.`, `{`, `}`) or end-of-input.

- **Whitespace.** Mandatory between tokens, except immediately after
  operator characters (`+`, `-`, `@`, `!`, `?`, `.`) and around braces
  (`{`, `}`).

- **Anonymous ID.** The `id` rule includes `_` for anonymous objects
  (valid only in `upsert`).

- **Qualified IDs.** The `id` rule includes `client:id` form for
  cross-client references used by daemons and the orchestrator.

- **Bare values and operators.** Operator characters (`+`, `-`, `@`,
  `!`, `?`, `.`) cannot start a bare value — they are always parsed
  as command operators at a token boundary. They are valid mid-value
  (e.g. `notes-app`, `w-64`, `a+b`, `com.example.foo`). To use a
  value that starts with an operator character, quote it: `value="+5"`.

- **Event dispatch.** The `event` rule tries `ack` as a keyword first,
  then falls back to `other_event` for all other event types (both
  built-in like `click` and third-party like `com.example.spell-check`).

- **Request dispatch.** The `request` rule tries `claim`, `unclaim`,
  `observe`, and `unobserve` as keywords first, then falls back to
  `other_req` (including `expand` and custom request types).

- **Response dispatch.** The `response` rule tries `expand` first
  (which requires a mandatory children body), then falls back to
  `other_res` (with optional children).

- **Dot-qualified types.** `.` is an operator at token start but is
  valid within bare words (mid-token). So `com.example.foo` tokenizes
  as a single word, while `.expand` tokenizes as `.` operator + `expand`.
