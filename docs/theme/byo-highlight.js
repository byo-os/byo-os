/*
  BYO/OS Protocol — highlight.js language definition
  License: BSD-3-Clause (same as highlight.js)

  Full grammar (from GRAMMAR.md):

    batch      = ws? (command ws?)*
    command    = upsert / destroy / patch / event / request / response

    upsert     = '+' type id prop* children?
    destroy    = '-' type id
    patch      = '@' type id prop* children?
    children   = '{' ws? (command ws?)* '}'

    event      = '!' (ack / other_event)
    ack        = 'ack' type seqnum prop*
    other_event = type seqnum id prop*

    request      = '?' (claim / unclaim / observe / unobserve / other_req)
    claim        = 'claim' seqnum types
    unclaim      = 'unclaim' seqnum types
    observe      = 'observe' seqnum types
    unobserve    = 'unobserve' seqnum types
    types        = type (',' type)*
    other_req    = type seqnum id prop*

    response   = '.' (expand_res / other_res)
    expand_res = 'expand' seqnum children
    other_res  = type seqnum prop* children?

    prop       = '~' name / name '=' value / name
    value      = dqstring / sqstring / bare
    dqstring   = '"' ([^"\\] / escape)* '"'
    sqstring   = "'" ([^'\\] / escape)* "'"
    escape     = '\\' ["'\\/nrt0]
    bare       = [^\s{}="'~\\,+\-@!?.] [^\s{}="'~\\,]*

    type       = [a-zA-Z][a-zA-Z0-9._-]*
    id         = [a-zA-Z_][a-zA-Z0-9_:-]*
    name       = [a-zA-Z][a-zA-Z0-9_-]*
    seqnum     = [0-9]+

  Operators (+, -, @, !, ?, .) only appear at token boundaries.
  They are valid mid-value (e.g. save-root, w-64, com.example.foo).
*/
hljs.registerLanguage("byo", function () {
  "use strict";
  return function (hljs) {
    // Characters valid inside identifiers and bare values — operators
    // preceded by any of these are mid-token, not command starts.
    var NOT_BOUNDARY = "[a-zA-Z0-9_\\-.]";

    return {
      name: "BYO",
      aliases: ["byo-os"],
      case_insensitive: false,
      contains: [
        // -- Comments --
        hljs.COMMENT("//", "$", { relevance: 0 }),
        hljs.C_BLOCK_COMMENT_MODE,

        // -- APC escape framing --
        // \e_B (BYO protocol) or \e_G (Kitty graphics)
        {
          className: "meta",
          begin: /\\e_[BG]/,
          relevance: 10
        },
        // \e\\ (String Terminator)
        {
          className: "meta",
          begin: /\\e\\\\/,
          relevance: 0
        },

        // -- Property removal: ~name --
        {
          className: "keyword",
          begin: /~[a-zA-Z][a-zA-Z0-9_-]*/,
          relevance: 5
        },

        // -- Commands: op + type as one unit --
        // Negative lookbehind ensures operators only match at token
        // boundaries — not mid-identifier (e.g. "-root" in "save-root").
        {
          className: "keyword",
          begin: "(?<!" + NOT_BOUNDARY + ")[+\\-@!?.][a-zA-Z][a-zA-Z0-9._-]*",
          relevance: 10
        },

        // -- Children delimiters --
        {
          className: "punctuation",
          begin: /[{}]/,
          relevance: 0
        },

        // -- Qualified IDs: client:id --
        {
          className: "symbol",
          begin: /[a-zA-Z][a-zA-Z0-9_-]*:[a-zA-Z][a-zA-Z0-9_-]*/,
          relevance: 5
        },

        // -- Double-quoted strings --
        {
          className: "string",
          begin: '"',
          end: '"',
          contains: [hljs.BACKSLASH_ESCAPE],
          relevance: 0
        },

        // -- Single-quoted strings --
        {
          className: "string",
          begin: "'",
          end: "'",
          contains: [hljs.BACKSLASH_ESCAPE],
          relevance: 0
        },

        // -- Sequence numbers / numeric values --
        {
          className: "number",
          begin: /\b\d+\b/,
          relevance: 0
        },

        // -- Property key (before =) --
        {
          className: "attr",
          begin: /[a-zA-Z][a-zA-Z0-9_-]*(?==)/,
          relevance: 0
        },

        // -- Anonymous ID --
        {
          className: "literal",
          begin: /\b_\b/,
          relevance: 0
        }
      ]
    };
  };
}());
