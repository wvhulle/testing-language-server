; Node.js test runner discovery query
; Based on https://github.com/nvim-neotest/neotest-jest patterns
; Adapted for Node.js test runner syntax

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

; -- Tests --
; Matches: `test("test name", (t) => {})` or `it("test name", (t) => {})`
((call_expression
  function: (identifier) @func_name (#any-of? @func_name "test" "it")
  arguments: (arguments (string (string_fragment) @test.name) [(arrow_function) (function_expression)])
)) @test.definition
; Matches: `test("test name", { skip: true }, (t) => {})`
((call_expression
  function: (identifier) @func_name (#any-of? @func_name "test" "it")
  arguments: (arguments
    (string (string_fragment) @test.name)
    (object)
    [(arrow_function) (function_expression)]
  )
)) @test.definition
; Matches: `test("test name", async (t) => {})`
((call_expression
  function: (identifier) @func_name (#any-of? @func_name "test" "it")
  arguments: (arguments
    (string (string_fragment) @test.name)
    (arrow_function (identifier) @async (#eq? @async "async"))
  )
)) @test.definition
; Matches: `test("test name", (t, done) => {})`
((call_expression
  function: (identifier) @func_name (#any-of? @func_name "test" "it")
  arguments: (arguments
    (string (string_fragment) @test.name)
    [(arrow_function (formal_parameters (identifier) (identifier))) (function_expression)]
  )
)) @test.definition
