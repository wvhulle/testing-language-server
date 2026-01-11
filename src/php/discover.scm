; PHP test discovery query for PHPUnit
; From https://github.com/olimorris/neotest-phpunit/blob/bbd79d95e927ccd16f0e1d765060058d34838e2e/lua/neotest-phpunit/init.lua#L111
; License: https://github.com/olimorris/neotest-phpunit/blob/bbd79d95e927ccd16f0e1d765060058d34838e2e/LICENSE

((class_declaration
  name: (name) @namespace.name (#match? @namespace.name "Test")
)) @namespace.definition

((method_declaration
  (attribute_list
    (attribute_group
        (attribute) @test_attribute (#match? @test_attribute "Test")
    )
  )
  (
    (visibility_modifier)
    (name) @test.name
  ) @test.definition
 ))

((method_declaration
  (name) @test.name (#match? @test.name "test")
)) @test.definition

(((comment) @test_comment (#match? @test_comment "\\@test") .
  (method_declaration
    (name) @test.name
  ) @test.definition
))
