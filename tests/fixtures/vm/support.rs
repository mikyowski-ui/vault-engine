pub(crate) use crate::fixture_assets as assets;
pub fn program() -> Vec<u8> {
    assets::sparse(include_str!("program.int.hex"))
}
pub fn push(value: i32) -> Vec<u8> {
    let mut bytes = vec![0xc0, 1];
    bytes.extend(value.to_be_bytes());
    bytes
}
pub fn op(code: u16) -> Vec<u8> {
    code.to_be_bytes().to_vec()
}
/// A host-callable zero-argument body returning a placeholder and a conditional
/// stream that exports a variable (proving evaluation is not pure), then returns.
pub fn scheduled_program(predicate: i32) -> (Vec<u8>, u32) {
    let mut bytes = program();
    bytes.truncate(0xfc);
    for instruction in [
        op(0x802b),
        push(5),
        op(0x800d),
        op(0x802a),
        op(0x800c),
        op(0x8029),
        op(0x801c),
    ] {
        bytes.extend(instruction);
    }
    let condition = bytes.len() as u32;
    for instruction in [push(6), op(0x8016), push(predicate), op(0x801c)] {
        bytes.extend(instruction);
    }
    (bytes, condition)
}
