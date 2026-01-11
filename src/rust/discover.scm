; Rust test discovery query
; From https://github.com/rouge8/neotest-rust
(
  (attribute_item
    [
      (attribute (identifier) @macro_name)
      (attribute
        [
          (identifier) @macro_name
          (scoped_identifier name: (identifier) @macro_name)
        ]
      )
    ]
  )
  [(attribute_item (attribute (identifier))) (line_comment)]*
  .
  (function_item name: (identifier) @test.name) @test.definition
  (#any-of? @macro_name "test" "rstest" "case")
)
(mod_item name: (identifier) @namespace.name)? @namespace.definition
