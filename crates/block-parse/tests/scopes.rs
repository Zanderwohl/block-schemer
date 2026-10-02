use block_parse::ast::{Expr, ProblemCode, Stmt};
use block_parse::edit::Target;
use block_parse::language::LanguageError;
use block_parse::{Block, BlockId, Declaration, Language, Program, Slot, Stack, Validators};

const LANGUAGE: &str = r#"Language(
    name: "s",
    file: (extension: "s"),
    types: {
        "name": (shape: Square, literal: Text),
        "value": (literal: Text),
        "pair": (),
    },
    blocks: [
        (id: "get", name: "Get", kind: Reporter("value"), spec: "{name:name}"),
        (
            id: "fn", name: "Fn", kind: Reporter("value"), spec: "fn {params:name*} {body:value}",
            scope: (declares: ["params"], over: ["body"], reference: "get"),
        ),
        (
            id: "let", name: "Let", kind: Reporter("value"), spec: "let {pairs:pair*} {body:value}",
            scope: (declares: ["pairs"], over: ["body"]),
        ),
        (
            id: "pair", name: "Pair", kind: Reporter("pair"), spec: "{name:name} = {init:value}",
            scope: (declares: ["name"], reference: "get"),
        ),
        (
            id: "for", name: "For", spec: "for {each:name} [body]",
            scope: (declares: ["each"], over: ["body"], reference: "get"),
        ),
        (id: "say", name: "Say", spec: "say {what:value}"),
    ],
)"#;

fn language() -> Language {
    Language::from_ron(LANGUAGE, &Validators::new()).unwrap()
}

fn problems(text: &str) -> Vec<String> {
    match Language::from_ron(text, &Validators::new()) {
        Err(LanguageError::Invalid(problems)) => problems.into_iter().map(|p| p.message).collect(),
        other => panic!("{other:?}"),
    }
}

/// A stack of one `say` whose slot holds `fn x -> <reference to x>`.
fn said_function(language: &Language) -> (Program, BlockId, BlockId) {
    let mut program = Program::new(language);
    let mut function = program.instantiate(language, "fn").unwrap();
    function.lists.insert("params".into(), vec![literal("x")]);
    let function_id = function.id;
    let say = program.instantiate(language, "say").unwrap();
    let say_id = say.id;
    program.stacks.push(Stack {
        pos: [0.0, 0.0],
        blocks: vec![say],
    });
    plug(&mut program, language, function, say_id, Slot::input("what"));
    let reference = program.reference(language, &param(function_id, 0)).unwrap();
    let reference_id = reference.id;
    plug(&mut program, language, reference, function_id, Slot::input("body"));
    (program, function_id, reference_id)
}

fn literal(text: &str) -> block_parse::Input {
    block_parse::Input {
        literal: Some(text.into()),
        block: None,
    }
}

fn param(function: BlockId, index: usize) -> Declaration {
    Declaration {
        block: function,
        slot: Slot::item("params", index),
    }
}

fn plug(program: &mut Program, language: &Language, block: Block, parent: BlockId, slot: Slot) {
    let fragment = block_parse::Fragment { blocks: vec![block] };
    program.attach(language, fragment, Target::Input { parent, slot }).unwrap();
}

fn name(program: &Program, reference: BlockId) -> Option<&str> {
    program.find(reference)?.inputs.get("name")?.literal.as_deref()
}

fn codes(program: &Program, language: &Language) -> Vec<ProblemCode> {
    program.ast(language).problems().iter().map(|problem| problem.code).collect()
}

#[test]
fn a_reference_mirrors_its_declaration_and_follows_its_renames() {
    let language = language();
    let (mut program, function, reference) = said_function(&language);
    assert_eq!(name(&program, reference), Some("x"));
    assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());

    assert!(program.set_literal(function, &Slot::item("params", 0), "y".into()));
    assert_eq!(name(&program, reference), Some("y"));
    assert!(!program.set_literal(reference, &Slot::input("name"), "z".into()), "a reference is not edited");

    let ast = program.ast(&language);
    let Stmt::Node(say) = &ast.scripts[0].body[0] else { panic!() };
    let Some(Expr::Node(function)) = say.arg("what") else { panic!() };
    let Some(Expr::Node(body)) = function.arg("body") else { panic!() };
    assert_eq!(body.refers, Some(param(function.id, 0)));
}

#[test]
fn only_a_named_declaration_gives_a_reference() {
    let language = language();
    let mut program = Program::new(&language);
    let mut function = program.instantiate(&language, "fn").unwrap();
    function.lists.insert("params".into(), vec![literal("  ")]);
    function.inputs.insert("body".into(), literal("x"));
    let id = function.id;
    program.stacks.push(Stack {
        pos: [0.0, 0.0],
        blocks: vec![function],
    });
    assert!(program.reference(&language, &param(id, 0)).is_none(), "blank");
    let body = Declaration {
        block: id,
        slot: Slot::input("body"),
    };
    assert!(program.reference(&language, &body).is_none(), "declares nothing");
}

#[test]
fn a_reference_outside_its_scope_is_an_empty_slot() {
    let language = language();
    let (mut program, _, reference) = said_function(&language);
    let fragment = program.detach(reference).unwrap();
    let say = program.stacks[0].blocks[0].id;
    let outside = program.instantiate(&language, "say").unwrap();
    let outside_id = outside.id;
    program.attach(&language, block_parse::Fragment { blocks: vec![outside] }, Target::After(say)).unwrap();
    program.attach(&language, fragment, Target::Input { parent: outside_id, slot: Slot::input("what") }).unwrap();

    let ast = program.ast(&language);
    let found = ast.problems();
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].code, ProblemCode::OutOfScope);
    assert_eq!(found[0].slot, Some((outside_id, Slot::input("what"))));
    assert!(found[0].recovered.is_none());
    assert!(found[0].message.contains("outside"), "{}", found[0].message);
    assert!(!ast.is_clean());

    let alone = program.script_at(&language, reference).unwrap();
    assert!(matches!(&alone.body[..], [Stmt::Problem(p)] if p.code == ProblemCode::OutOfScope));
}

#[test]
fn running_part_of_a_scope_takes_its_references_out_of_it() {
    let language = language();
    let (program, _, reference) = said_function(&language);
    let alone = program.script_at(&language, reference).unwrap();
    assert!(matches!(&alone.body[..], [Stmt::Problem(p)] if p.code == ProblemCode::OutOfScope));
}

#[test]
fn a_blank_name_is_unnamed_in_its_declaration_and_its_references() {
    let language = language();
    let (mut program, function, reference) = said_function(&language);
    program.find_mut(function).unwrap().lists.get_mut("params").unwrap().push(literal("y"));
    let second = program.reference(&language, &param(function, 1)).unwrap();
    let say = program.instantiate(&language, "say").unwrap();
    let say_id = say.id;
    let outer = program.stacks[0].blocks[0].id;
    program.attach(&language, block_parse::Fragment { blocks: vec![say] }, Target::After(outer)).unwrap();
    plug(&mut program, &language, second, say_id, Slot::input("what"));
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope], "the second is outside");

    // Blank but still an item: the declaration and the reference both say so.
    assert!(program.set_literal(function, &Slot::item("params", 0), " ".into()));
    assert_eq!(name(&program, reference), Some(" "));
    let ast = program.ast(&language);
    let found = ast.problems();
    assert_eq!(
        found.iter().map(|p| p.code).collect::<Vec<_>>(),
        [ProblemCode::Unnamed, ProblemCode::Unnamed, ProblemCode::OutOfScope]
    );
    assert_eq!(found[0].message, "Unnamed params 1 has no name", "the body comes before the list");
    assert!(found[1].message.contains("needs a name"), "{}", found[1].message);
    assert!(!ast.is_clean());

    // Emptied off the end: the reference keeps its index and comes back
    // when the slot is named again.
    program.find_mut(function).unwrap().lists.get_mut("params").unwrap().truncate(1);
    assert!(program.set_literal(function, &Slot::item("params", 0), String::new()));
    assert!(!program.find(function).unwrap().lists.contains_key("params"));
    assert_eq!(codes(&program, &language), [ProblemCode::Unnamed, ProblemCode::OutOfScope]);
    assert!(program.set_literal(function, &Slot::item("params", 0), "z".into()));
    assert_eq!(name(&program, reference), Some("z"));
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope]);
}

#[test]
fn a_hole_in_a_declaring_list_is_unnamed_too() {
    let language = language();
    let (mut program, function, _) = said_function(&language);
    program.find_mut(function).unwrap().lists.get_mut("params").unwrap().push(literal("y"));
    assert!(program.set_literal(function, &Slot::item("params", 0), String::new()));
    assert!(program.find(function).unwrap().lists["params"][0].is_hole(), "kept, as `y` follows it");
    let ast = program.ast(&language);
    let messages: Vec<_> = ast.problems().iter().map(|p| (p.code, p.message.as_str())).collect();
    assert_eq!(
        messages,
        [
            (ProblemCode::Unnamed, "Unnamed params 1 has no name"),
            (ProblemCode::Unnamed, "item 1 of `params` of `Fn` needs a name"),
        ]
    );
}

#[test]
fn a_blank_single_input_is_named_by_its_hint_alone() {
    let language = language();
    let pair = language.block("pair").unwrap();
    assert_eq!(pair.unnamed(&Slot::input("name")), "Unnamed name");
    assert_eq!(pair.unnamed(&Slot::item("name", 2)), "Unnamed name 3", "or its hint and place in a list");
}

#[test]
fn a_reference_to_a_declaration_that_is_gone_says_so() {
    let language = language();
    let (mut program, function, reference) = said_function(&language);
    let fragment = program.detach(reference).unwrap();
    program.attach(&language, fragment, Target::Free { pos: [0.0, 100.0] }).unwrap();
    program.remove(function);
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope]);
    assert!(program.ast(&language).problems()[0].message.contains("gone"));
}

#[test]
fn a_global_name_may_be_used_anywhere() {
    let language = Language::from_ron(
        &LANGUAGE.replace(
            r#"(id: "say", name: "Say", spec: "say {what:value}"),"#,
            r#"(id: "say", name: "Say", spec: "say {what:value}"),
            (
                id: "def", name: "Def", spec: "def {name:name} {params:name*} [body]",
                scope: (declares: ["name", "params"], over: ["body"], global: ["name"], reference: "get"),
            ),"#,
        ),
        &Validators::new(),
    )
    .unwrap();
    let mut program = Program::new(&language);
    let mut def = program.instantiate(&language, "def").unwrap();
    def.inputs.insert("name".into(), literal("f"));
    def.lists.insert("params".into(), vec![literal("x")]);
    let def_id = def.id;
    let say = program.instantiate(&language, "say").unwrap();
    let say_id = say.id;
    program.stacks.push(Stack { pos: [0.0, 0.0], blocks: vec![def] });
    program.stacks.push(Stack { pos: [0.0, 100.0], blocks: vec![say] });
    let f = Declaration { block: def_id, slot: Slot::input("name") };
    let call = program.reference(&language, &f).unwrap();
    plug(&mut program, &language, call, say_id, Slot::input("what"));
    assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());

    let x = Declaration { block: def_id, slot: Slot::item("params", 0) };
    let stray = program.reference(&language, &x).unwrap();
    program.attach(&language, block_parse::Fragment { blocks: vec![stray] }, Target::Free { pos: [0.0, 200.0] }).unwrap();
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope], "parameters stay local");
}

#[test]
fn a_binding_hands_its_name_to_the_body_of_the_block_holding_it() {
    let language = language();
    let mut program = Program::new(&language);
    let mut pair = program.instantiate(&language, "pair").unwrap();
    pair.inputs.insert("name".into(), literal("a"));
    let pair_id = pair.id;
    let mut scope = program.instantiate(&language, "let").unwrap();
    scope.lists.insert(
        "pairs".into(),
        vec![block_parse::Input {
            literal: None,
            block: Some(Box::new(pair)),
        }],
    );
    let scope_id = scope.id;
    program.stacks.push(Stack {
        pos: [0.0, 0.0],
        blocks: vec![scope],
    });
    let a = Declaration {
        block: pair_id,
        slot: Slot::input("name"),
    };
    let in_body = program.reference(&language, &a).unwrap();
    plug(&mut program, &language, in_body, scope_id, Slot::input("body"));
    assert!(program.ast(&language).is_clean(), "{:#?}", program.ast(&language).problems());

    // As in a `let`, the initializers are outside.
    let in_init = program.reference(&language, &a).unwrap();
    plug(&mut program, &language, in_init, pair_id, Slot::input("init"));
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope]);
}

#[test]
fn a_branch_can_be_a_scope() {
    let language = language();
    let mut program = Program::new(&language);
    let mut each = program.instantiate(&language, "for").unwrap();
    each.inputs.insert("each".into(), literal("i"));
    let each_id = each.id;
    let inside = program.instantiate(&language, "say").unwrap();
    let after = program.instantiate(&language, "say").unwrap();
    let (inside_id, after_id) = (inside.id, after.id);
    each.branches.insert("body".into(), vec![inside]);
    program.stacks.push(Stack {
        pos: [0.0, 0.0],
        blocks: vec![each, after],
    });
    let i = Declaration {
        block: each_id,
        slot: Slot::input("each"),
    };
    let reference = program.reference(&language, &i).unwrap();
    plug(&mut program, &language, reference, inside_id, Slot::input("what"));
    assert!(program.ast(&language).is_clean());
    let reference = program.reference(&language, &i).unwrap();
    plug(&mut program, &language, reference, after_id, Slot::input("what"));
    assert_eq!(codes(&program, &language), [ProblemCode::OutOfScope]);
}

#[test]
fn a_duplicate_scope_refers_to_its_own_declarations() {
    let language = language();
    let (mut program, function, _) = said_function(&language);
    let copy = program.duplicate(function).unwrap();
    let copy_id = copy.blocks[0].id;
    let body = copy.blocks[0].inputs["body"].block.as_deref().unwrap();
    assert_eq!(body.refers, Some(param(copy_id, 0)));
    program.attach(&language, copy, Target::Free { pos: [0.0, 100.0] }).unwrap();
    assert!(program.ast(&language).is_clean());

    assert!(program.set_literal(copy_id, &Slot::item("params", 0), "w".into()));
    let names: Vec<_> = program
        .ast(&language)
        .scripts
        .iter()
        .map(|script| format!("{:?}", script.body))
        .collect();
    assert!(names[0].contains("\"x\"") && names[1].contains("\"w\""), "{names:#?}");
}

#[test]
fn loading_brings_references_in_line_with_their_declarations() {
    let language = language();
    let (mut program, _, reference) = said_function(&language);
    program.find_mut(reference).unwrap().inputs.get_mut("name").unwrap().literal = Some("stale".into());
    let loaded = Program::from_ron(&program.to_ron()).unwrap();
    assert_eq!(name(&loaded, reference), Some("x"));
}

#[test]
fn loading_keeps_a_references_name_when_its_declaration_cannot_give_one() {
    let language = language();
    let (mut program, function, reference) = said_function(&language);
    let stray = program.reference(&language, &param(function, 0)).unwrap();
    let stray_id = stray.id;
    program.attach(&language, block_parse::Fragment { blocks: vec![stray] }, Target::Free { pos: [0.0, 100.0] }).unwrap();

    // A block plugged over the name hides it but does not empty it.
    let cover = program.instantiate(&language, "get").unwrap();
    program.find_mut(function).unwrap().lists.get_mut("params").unwrap()[0].block = Some(Box::new(cover));
    let loaded = Program::from_ron(&program.to_ron()).unwrap();
    assert_eq!(name(&loaded, reference), Some("x"));

    program.remove(program.stacks[0].blocks[0].id);
    let loaded = Program::from_ron(&program.to_ron()).unwrap();
    assert_eq!(name(&loaded, stray_id), Some("x"), "its declaring block is gone");
}

#[test]
fn scopes_are_checked_when_the_language_compiles() {
    let bad = LANGUAGE
        .replace(r#"declares: ["params"]"#, r#"declares: ["nope"]"#)
        .replace(r#"over: ["body"], reference: "get"),
        ),
        (
            id: "let""#, r#"over: ["elsewhere"], reference: "get"),
        ),
        (
            id: "let""#)
        .replace(r#"scope: (declares: ["name"], reference: "get")"#, r#"scope: (declares: ["name"])"#)
        .replace(r#"spec: "for {each:name} [body]",
            scope: (declares: ["each"], over: ["body"], reference: "get")"#, r#"spec: "for {each:value} [body]",
            scope: (declares: ["each"], over: ["each"], global: ["other"], reference: "say")"#);
    let found = problems(&bad);
    assert_eq!(found.len(), 6, "{found:#?}");
    for expected in ["`nope`", "`elsewhere`", "no reference", "`say` must be a reporter", "both declares", "global `other`"] {
        assert!(found.iter().any(|p| p.contains(expected)), "{expected}: {found:#?}");
    }

    let mismatch = LANGUAGE.replace("for {each:name} [body]", "for {each:value} [body]");
    assert_eq!(problems(&mismatch), ["`each` is value, but the reference's input is name"]);
}
