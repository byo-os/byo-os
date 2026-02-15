# BYO/OS Command Grammar

PEG grammar for the BYO/OS command language. The scanner extracts the
payload between `ESC _ B` and `ESC \`; this grammar defines the
structure within that payload.

```peg
# -- Top level --

batch      = whitespace? (command whitespace?)*
command    = upsert | destroy | patch | event

# -- Object operations --

upsert     = '+' type id prop* children?
destroy    = '-' type id
patch      = '@' type id prop* children?
children   = '{' whitespace? (command whitespace?)* '}'

# -- Events --

event      = '!' (ack | sub | unsub | other)
ack        = 'ack' type seqnum prop*
sub        = 'sub' seqnum type
unsub      = 'unsub' seqnum type
other      = type seqnum id prop*

# -- Properties --

prop       = '~' name | name '=' value | name
value      = dqstring | sqstring | bare

# -- Strings --

dqstring   = '"' dqchar* '"'
dqchar     = [^"\\] | escape
sqstring   = "'" sqchar* "'"
sqchar     = [^'\\] | escape
escape     = '\\' ["'\\/nrt0]
bare       = [^\s{}="'~\\+\-@!] [^\s{}="'~\\]*

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

- **Children targets.** Only `upsert` (`+`) and `patch` (`@`) can
  have children blocks.

- **Greedy props.** `prop*` is delimited by whitespace. The parser
  greedily consumes props until it hits a character that starts a
  new command (`+`, `-`, `@`, `!`, `{`, `}`) or end-of-input.

- **Whitespace.** Mandatory between tokens, except immediately after
  operator characters (`+`, `-`, `@`, `!`) and around braces
  (`{`, `}`).

- **Anonymous ID.** The `id` rule includes `_` for anonymous objects
  (valid only in `upsert`).

- **Qualified IDs.** The `id` rule includes `client:id` form for
  cross-client references used by daemons and the orchestrator.

- **Bare values and operators.** Operator characters (`+`, `-`, `@`,
  `!`) cannot start a bare value — they are always parsed as command
  operators at a token boundary. They are valid mid-value (e.g.
  `notes-app`, `w-64`, `a+b`). To use a value that starts with an
  operator character, quote it: `value="+5"`.

- **Event dispatch.** The `event` rule tries `ack`, `sub`, `unsub`
  as keywords first, then falls back to `other` for all other event
  types (both built-in like `click` and third-party like
  `com.example.spell-check`).
