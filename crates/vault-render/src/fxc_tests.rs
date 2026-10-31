//! What FXC, the Direct3D 12 compiler, makes of the shaders. naga's HLSL
//! writer exists only in Windows builds, so this reads naga's IR for the
//! constructs that come out badly there:
//!
//! - a negated or `abs`-ed integer becomes `asint(-asuint(x))`, which FXC
//!   warns about as a unary negate of an unsigned value;
//! - a signed `/` or `%` becomes a helper with a signed divide, which it warns
//!   about as slow;
//! - a `switch` case that ends in `discard` is written with no `break`, and
//!   FXC requires a non-empty case to end in `break` or `return`.
use wgpu::naga::{
    self, BinaryOperator, Block, Expression, Function, Handle, MathFunction, Module, ScalarKind,
    Statement, UnaryOperator,
    valid::{FunctionInfo, ModuleInfo},
};

const SHADERS: [(&str, &str); 3] = [
    ("indexed.wgsl", include_str!("indexed.wgsl")),
    ("layers.wgsl", include_str!("layers.wgsl")),
    ("instanced.wgsl", include_str!("layers/instanced.wgsl")),
];

fn parsed(source: &str) -> (Module, ModuleInfo) {
    let module = naga::front::wgsl::parse_str(source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    (module, info)
}
/// Whether `block` ends in `discard`, directly or in a nested block.
fn ends_in_discard(block: &Block) -> bool {
    match block.last() {
        Some(Statement::Kill) => true,
        Some(Statement::Block(inner)) => ends_in_discard(inner),
        _ => false,
    }
}
fn cases(block: &Block, name: &str, found: &mut Vec<String>) {
    for statement in block.iter() {
        match statement {
            Statement::Block(inner) => cases(inner, name, found),
            Statement::If { accept, reject, .. } => {
                cases(accept, name, found);
                cases(reject, name, found);
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                cases(body, name, found);
                cases(continuing, name, found);
            }
            Statement::Switch { cases: list, .. } => {
                for case in list {
                    if ends_in_discard(&case.body) {
                        found.push(format!("{name}: a switch case ends in discard"));
                    }
                    cases(&case.body, name, found);
                }
            }
            _ => {}
        }
    }
}
fn function(
    module: &Module,
    body: &Function,
    info: &FunctionInfo,
    name: &str,
    found: &mut Vec<String>,
) {
    let kind = |expression: Handle<Expression>| {
        let inner = info[expression].ty.inner_with(&module.types);
        inner.scalar_kind()
    };
    for (_, expression) in body.expressions.iter() {
        match *expression {
            Expression::Unary {
                op: UnaryOperator::Negate,
                expr,
            } if kind(expr) == Some(ScalarKind::Sint) => {
                found.push(format!("{name}: a negated integer"));
            }
            Expression::Math {
                fun: MathFunction::Abs,
                arg,
                ..
            } if kind(arg) == Some(ScalarKind::Sint) => {
                found.push(format!("{name}: abs of an integer"));
            }
            Expression::Binary {
                op: BinaryOperator::Divide | BinaryOperator::Modulo,
                left,
                ..
            } if kind(left) == Some(ScalarKind::Sint) => {
                found.push(format!("{name}: a signed divide"));
            }
            _ => {}
        }
    }
    cases(&body.body, name, found);
}
/// Everything in `source` that FXC warns about or refuses.
fn findings(source: &str) -> Vec<String> {
    let (module, info) = parsed(source);
    let mut found = Vec::new();
    for (handle, body) in module.functions.iter() {
        let name = body.name.as_deref().unwrap_or("?");
        function(&module, body, &info[handle], name, &mut found);
    }
    for (index, entry) in module.entry_points.iter().enumerate() {
        let info = info.get_entry_point(index);
        function(&module, &entry.function, info, &entry.name, &mut found);
    }
    found
}

#[test]
fn every_shader_is_free_of_what_fxc_warns_about_or_refuses() {
    for (name, source) in SHADERS {
        assert_eq!(findings(source), Vec::<String>::new(), "{name}");
    }
}

#[test]
fn the_check_finds_each_construct() {
    for (body, expected) in [
        ("return abs(x);", "f: abs of an integer"),
        ("return -x;", "f: a negated integer"),
        ("return x % 4;", "f: a signed divide"),
        ("return x / 4;", "f: a signed divide"),
    ] {
        let source = format!("fn f(x: i32) -> i32 {{ {body} }}");
        assert_eq!(findings(&source), [expected], "{body}");
    }
    let source = "@fragment fn f(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
        switch u32(at.x) { case 0u: { } default: { discard; } }
        return at;
    }";
    assert_eq!(findings(source), ["f: a switch case ends in discard"]);
    // Unsigned divides, float negation and float `abs` stay allowed: FXC
    // takes them, and `item.hex % 200u` has no power of two to shift by.
    let source = "fn f(x: u32, y: f32) -> f32 { return f32(x / 3u + x % 7u) + abs(-y); }";
    assert_eq!(findings(source), Vec::<String>::new());
}

/// The instanced shader's replacements, against what they replaced, over
/// the whole i32 range's edges and every value a canvas reaches.
#[test]
fn the_replacements_keep_the_exact_results() {
    let edges = [i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX];
    for value in (-70_000..70_000).chain(edges) {
        // `((x % 4) + 4) % 4`, with WGSL's truncating `%`.
        assert_eq!(((value % 4) + 4) % 4, value & 3, "{value}");
        // `abs` wraps at i32::MIN in WGSL, as `0 - value` does.
        let replaced = if value < 0 {
            0i32.wrapping_sub(value)
        } else {
            value
        };
        assert_eq!(value.wrapping_abs(), replaced, "{value}");
    }
    for b in 0..16u32 {
        assert_eq!((b / 4, b % 4), (b >> 2, b & 3), "{b}");
    }
}
