; Jest/Vitest test discovery query
; From https://github.com/nvim-neotest/neotest-jest/blob/514fd4eae7da15fd409133086bb8e029b65ac43f/lua/neotest-jest/init.lua#L162
; License: https://github.com/nvim-neotest/neotest-jest/blob/514fd4eae7da15fd409133086bb8e029b65ac43f/LICENSE.md

; -- Namespaces --
; Matches: `describe('context', () => {})`
((call_expression
  function: (identifier) @func_name (#eq? @func_name "describe")
  arguments: (arguments (string (string_fragment) @namespace.name) (arrow_function))
)) @namespace.definition
; Matches: `describe('context', function() {})`
((call_expression
  function: (identifier) @func_name (#eq? @func_name "describe")
  arguments: (arguments (string (string_fragment) @namespace.name) (function_expression))
)) @namespace.definition
; Matches: `describe.only('context', () => {})`
((call_expression
  function: (member_expression
    object: (identifier) @func_name (#any-of? @func_name "describe")
  )
  arguments: (arguments (string (string_fragment) @namespace.name) (arrow_function))
)) @namespace.definition
; Matches: `describe.only('context', function() {})`
((call_expression
  function: (member_expression
    object: (identifier) @func_name (#any-of? @func_name "describe")
  )
  arguments: (arguments (string (string_fragment) @namespace.name) (function_expression))
)) @namespace.definition
; Matches: `describe.each(['data'])('context', () => {})`
((call_expression
  function: (call_expression
    function: (member_expression
      object: (identifier) @func_name (#any-of? @func_name "describe")
    )
  )
  arguments: (arguments (string (string_fragment) @namespace.name) (arrow_function))
)) @namespace.definition
; Matches: `describe.each(['data'])('context', function() {})`
((call_expression
  function: (call_expression
    function: (member_expression
      object: (identifier) @func_name (#any-of? @func_name "describe")
    )
  )
  arguments: (arguments (string (string_fragment) @namespace.name) (function_expression))
)) @namespace.definition

; -- Tests --
; Matches: `test('test') / it('test')`
((call_expression
  function: (identifier) @func_name (#any-of? @func_name "it" "test")
  arguments: (arguments (string (string_fragment) @test.name) [(arrow_function) (function_expression)])
)) @test.definition
; Matches: `test.only('test') / it.only('test')`
((call_expression
  function: (member_expression
    object: (identifier) @func_name (#any-of? @func_name "test" "it")
  )
  arguments: (arguments (string (string_fragment) @test.name) [(arrow_function) (function_expression)])
)) @test.definition
; Matches: `test.each(['data'])('test') / it.each(['data'])('test')`
((call_expression
  function: (call_expression
    function: (member_expression
      object: (identifier) @func_name (#any-of? @func_name "it" "test")
      property: (property_identifier) @each_property (#eq? @each_property "each")
    )
  )
  arguments: (arguments (string (string_fragment) @test.name) [(arrow_function) (function_expression)])
)) @test.definition
