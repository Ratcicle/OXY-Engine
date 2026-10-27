//! A small reader of the bytecode the engine's fixed compiler emits (version 9, type information
//! version 3 in Luau 0.740), enough for the load checks: top-level locals that some function
//! writes (captures by reference in the main function), closures that share a mutable upvalue
//! with their creator, the global paths a script imports and, for the lint of names, the
//! literal texts it passes to calls, uses as keys of a field, writes to one or compares a
//! parameter with. Any other version is refused, so an mlua update that changes the format fails
//! the contract test instead of passing unread.
use std::collections::{BTreeMap, BTreeSet};

/// The versions in the bytecode's first two bytes.
pub const VERSION: u8 = 9;
pub const TYPES_VERSION: u8 = 3;

// `LuauCaptureType` and the `LuauOpcode` numbers (Common/include/Luau/Bytecode.h).
const LCT_VAL: u32 = 0;
const LCT_REF: u32 = 1;
const LCT_UPVAL: u32 = 2;
const OP_LOADK: u32 = 5;
const OP_MOVE: u32 = 6;
const OP_GETGLOBAL: u32 = 7;
const OP_SETUPVAL: u32 = 10;
const OP_GETIMPORT: u32 = 12;
const OP_GETTABLE: u32 = 13;
const OP_SETTABLE: u32 = 14;
const OP_GETTABLEKS: u32 = 15;
const OP_SETTABLEKS: u32 = 16;
const OP_NEWCLOSURE: u32 = 19;
const OP_NAMECALL: u32 = 20;
const OP_CALL: u32 = 21;
const OP_JUMPIFEQ: u32 = 27;
const OP_JUMPIFNOTEQ: u32 = 30;
const OP_DUPCLOSURE: u32 = 64;
const OP_LOADKX: u32 = 66;
const OP_CAPTURE: u32 = 70;
const OP_JUMPXEQKS: u32 = 80;
const OP_GETUDATAKS: u32 = 83;
const OP_SETUDATAKS: u32 = 84;
const OP_NAMECALLUDATA: u32 = 85;
const OP_CALLFB: u32 = 87;
const OP_COUNT: u32 = 91;

/// `Luau::getOpLength` (Common/include/Luau/BytecodeUtils.h): the instructions an AUX word follows.
fn op_length(op: u32) -> usize {
    match op {
        7
        | 8
        | 12
        | 15
        | 16
        | 20
        | 27..=32
        | 53
        | 55
        | 58
        | 60
        | 66
        | 74
        | 75
        | 77..=80
        | 83..=88
        | 90 => 2,
        _ => 1,
    }
}

#[derive(Clone, Debug)]
enum Constant {
    Other,
    Text(Option<String>),
    Closure(u32),
}

#[derive(Clone, Debug)]
struct Local {
    name: Option<String>,
    start_pc: u32,
    end_pc: u32,
    register: u8,
}

#[derive(Clone, Debug, Default)]
struct Proto {
    parameters: u8,
    upvalues: u8,
    /// The function's name, as `on_input` for `function M.on_input`.
    name: Option<String>,
    code: Vec<u32>,
    constants: Vec<Constant>,
    children: Vec<u32>,
    line_gap: u8,
    line_offsets: Vec<u8>,
    lines: Vec<i32>,
    locals: Vec<Local>,
}

impl Proto {
    /// The source line of instruction `pc`, when the bytecode carries lines.
    fn line(&self, pc: usize) -> Option<u32> {
        let offset = *self.line_offsets.get(pc)?;
        let base = *self.lines.get(pc >> self.line_gap)?;
        u32::try_from(base + i32::from(offset)).ok()
    }
}

/// A parsed chunk: its functions and which one is the main.
#[derive(Clone, Debug)]
pub struct Module {
    protos: Vec<Proto>,
    main: usize,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    strings: Vec<String>,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.data.len());
        let end = end.ok_or("bytecode truncado")?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn var(&mut self) -> Result<u32, String> {
        u32::try_from(self.var64()?)
            .map_err(|_| "inteiro variável grande demais no bytecode".into())
    }
    fn var64(&mut self) -> Result<u64, String> {
        let (mut value, mut shift) = (0u64, 0);
        loop {
            let byte = self.u8()?;
            value |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                return Err("inteiro variável inválido no bytecode".into());
            }
        }
    }
    fn count(&mut self) -> Result<usize, String> {
        Ok(self.var()? as usize)
    }
    fn string(&mut self) -> Result<Option<String>, String> {
        match self.count()? {
            0 => Ok(None),
            id => self
                .strings
                .get(id - 1)
                .cloned()
                .map(Some)
                .ok_or_else(|| "texto fora da tabela do bytecode".into()),
        }
    }
}

/// Reads `bytes`, refusing any version but [`VERSION`] and [`TYPES_VERSION`].
pub fn parse(bytes: &[u8]) -> Result<Module, String> {
    let mut r = Reader {
        data: bytes,
        at: 0,
        strings: Vec::new(),
    };
    let version = r.u8()?;
    if version == 0 {
        return Err(String::from_utf8_lossy(&bytes[1..]).into_owned());
    }
    if version != VERSION {
        return Err(format!(
            "versão de bytecode {version} não suportada pelo leitor (esperado {VERSION})"
        ));
    }
    let types = r.u8()?;
    if types != TYPES_VERSION {
        return Err(format!(
            "versão de tipos {types} não suportada pelo leitor (esperado {TYPES_VERSION})"
        ));
    }
    for _ in 0..r.count()? {
        let length = r.count()?;
        let text = String::from_utf8_lossy(r.take(length)?).into_owned();
        r.strings.push(text);
    }
    // The userdata type remapping of type information version 3.
    while r.u8()? != 0 {
        r.string()?;
    }
    let count = r.count()?;
    let mut protos = Vec::with_capacity(count.min(bytes.len()));
    for _ in 0..count {
        protos.push(read_proto(&mut r)?);
    }
    let main = r.count()?;
    if main >= protos.len() {
        return Err("função principal fora da tabela do bytecode".into());
    }
    Ok(Module { protos, main })
}

fn read_proto(r: &mut Reader) -> Result<Proto, String> {
    let mut p = Proto::default();
    let _max_stack = r.u8()?;
    p.parameters = r.u8()?;
    p.upvalues = r.u8()?;
    let _vararg = r.u8()?;
    let _flags = r.u8()?;
    let types = r.count()?;
    r.take(types)?;
    let size = r.count()?;
    p.code = (0..size).map(|_| r.u32()).collect::<Result<_, _>>()?;
    for _ in 0..r.count()? {
        let constant = match r.u8()? {
            0 => Constant::Other,
            1 => {
                r.u8()?;
                Constant::Other
            }
            2 => {
                r.take(8)?;
                Constant::Other
            }
            3 => Constant::Text(r.string()?),
            4 => {
                r.u32()?;
                Constant::Other
            }
            5 => {
                for _ in 0..r.count()? {
                    r.var()?;
                }
                Constant::Other
            }
            6 => Constant::Closure(r.var()?),
            7 => {
                r.take(16)?;
                Constant::Other
            }
            8 => {
                for _ in 0..r.count()? {
                    r.var()?;
                    r.u32()?;
                }
                Constant::Other
            }
            9 => {
                r.u8()?;
                r.var64()?;
                Constant::Other
            }
            11 => {
                r.take(32)?;
                Constant::Other
            }
            kind => return Err(format!("constante de tipo {kind} desconhecida no bytecode")),
        };
        p.constants.push(constant);
    }
    p.children = (0..r.count()?).map(|_| r.var()).collect::<Result<_, _>>()?;
    let _line_defined = r.var()?;
    p.name = r.string()?;
    if r.u8()? != 0 {
        p.line_gap = r.u8()?;
        if p.line_gap > 31 {
            return Err("intervalo de linhas inválido no bytecode".into());
        }
        let mut last = 0u8;
        for _ in 0..size {
            last = last.wrapping_add(r.u8()?);
            p.line_offsets.push(last);
        }
        let intervals = if size == 0 {
            0
        } else {
            ((size - 1) >> p.line_gap) + 1
        };
        let mut line = 0i32;
        for _ in 0..intervals {
            line = line.wrapping_add(r.u32()? as i32);
            p.lines.push(line);
        }
    }
    if r.u8()? != 0 {
        for _ in 0..r.count()? {
            p.locals.push(Local {
                name: r.string()?,
                start_pc: r.var()?,
                end_pc: r.var()?,
                register: r.u8()?,
            });
        }
        for _ in 0..r.count()? {
            r.string()?;
        }
    }
    Ok(p)
}

/// A capture by reference in the main function: a top-level local that some function writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopLevelWrite {
    pc: usize,
    register: u8,
    /// The line of the function that captures it.
    pub line: Option<u32>,
    /// The line where that function, or one it creates, changes it; none when they only read
    /// it (the top changes it after declaring it) or the bytecode carries no lines.
    pub changed_at: Option<u32>,
}

/// Where a chunk gives a literal text that may name something: an argument, a key or the other
/// side of a comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Site {
    /// Argument `arg` (from 0) of a call of the global path `function` (`audio.play`).
    Call { function: String, arg: usize },
    /// Argument `arg` (from 0, after the receiver) of a call of the method `name` on a value
    /// read as the field `field` of another (`obj.animation:play`), or on any other value.
    Method {
        name: String,
        field: Option<String>,
        arg: usize,
    },
    /// A key, read or written, of a value read as the field `field` of another (`obj.attr.Vida`,
    /// `obj.attr["Vida"]`).
    Key { field: String },
    /// The value written to the key `key` of a value read as the field `field` of another
    /// (`obj.collider.surface = "Gelo"`).
    Set { field: String, key: String },
    /// A comparison of parameter `param` (from 0, `self` included) of a function named
    /// `function` (`action == "Pular"` in `M.on_input`).
    Compare { function: String, param: usize },
}

/// A literal text of a chunk at a site, with its line when the bytecode carries lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Literal {
    pub site: Site,
    pub text: String,
    pub line: Option<u32>,
}

/// What the load checks read from a chunk.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub top_level_writes: Vec<TopLevelWrite>,
    /// The global paths the chunk imports, as `math.floor`.
    pub imports: BTreeSet<String>,
    /// The lines each of `imports` is read at, when the bytecode carries lines.
    pub import_lines: BTreeMap<String, BTreeSet<u32>>,
    /// The functions (proto indexes, `lua_Debug.bytecodeid`) whose closures share a mutable
    /// upvalue with the function that creates them.
    pub stateful_protos: BTreeSet<u32>,
    /// The literal texts at the sites of [`Site`], function by function as the walk reaches
    /// them, each function's in the order of its code.
    pub literals: Vec<Literal>,
}

/// Walks the functions from the main one, checking that every capture follows a closure
/// creation and every opcode is known.
pub fn analyze(m: &Module) -> Result<Report, String> {
    let mut report = Report::default();
    let mut mutable: Vec<Vec<bool>> = m
        .protos
        .iter()
        .map(|p| vec![false; p.upvalues as usize])
        .collect();
    let mut seen = vec![false; m.protos.len()];
    let mut queue = vec![m.main];
    seen[m.main] = true;
    while let Some(id) = queue.pop() {
        let p = &m.protos[id];
        literals(p, &mut report.literals);
        let mut pc = 0;
        while pc < p.code.len() {
            let insn = p.code[pc];
            let op = insn & 0xff;
            if op >= OP_COUNT {
                return Err(format!("instrução {op} desconhecida no bytecode"));
            }
            let d = (insn as i32 >> 16) as usize;
            let child = match op {
                OP_NEWCLOSURE => Some(
                    *p.children
                        .get(d)
                        .ok_or("NEWCLOSURE fora da lista de filhos")? as usize,
                ),
                OP_DUPCLOSURE => match p.constants.get(d) {
                    Some(Constant::Closure(child)) => Some(*child as usize),
                    _ => return Err("DUPCLOSURE sem constante de função".into()),
                },
                OP_CAPTURE => return Err("CAPTURE fora de uma criação de função".into()),
                OP_GETIMPORT => {
                    let aux = *p.code.get(pc + 1).ok_or("GETIMPORT sem AUX")?;
                    let path = import_path(p, aux);
                    if let Some(line) = p.line(pc) {
                        let lines = report.import_lines.entry(path.clone()).or_default();
                        lines.insert(line);
                    }
                    report.imports.insert(path);
                    None
                }
                _ => None,
            };
            pc += op_length(op);
            let Some(child) = child else { continue };
            let upvalues = m
                .protos
                .get(child)
                .ok_or("função filha inexistente")?
                .upvalues;
            for k in 0..upvalues as usize {
                let capture = *p.code.get(pc + k).ok_or("CAPTURE ausente")?;
                if capture & 0xff != OP_CAPTURE {
                    return Err("criação de função sem os CAPTURE esperados".into());
                }
                let (kind, b) = ((capture >> 8) & 0xff, ((capture >> 16) & 0xff) as usize);
                let shared = match kind {
                    LCT_VAL => false,
                    LCT_REF => true,
                    LCT_UPVAL => *mutable[id].get(b).ok_or("CAPTURE de upvalue inexistente")?,
                    _ => return Err(format!("CAPTURE de tipo {kind} desconhecido")),
                };
                if kind == LCT_REF && id == m.main {
                    report.top_level_writes.push(TopLevelWrite {
                        pc: pc - 1,
                        register: b as u8,
                        line: p.line(pc - 1),
                        changed_at: upvalue_write(m, child, k, 0),
                    });
                }
                mutable[child][k] |= shared;
            }
            pc += upvalues as usize;
            if !seen[child] {
                seen[child] = true;
                queue.push(child);
            }
        }
    }
    // A function is created at one place and queued only after its creator was read, so its
    // flags are final before its own children read them.
    for (id, flags) in mutable.iter().enumerate() {
        if id != m.main && flags.iter().any(|&f| f) {
            report.stateful_protos.insert(id as u32);
        }
    }
    Ok(report)
}

/// The line of the first write of the upvalue `upvalue` of the function `id`, in its code or in
/// that of the functions it creates that capture it, when the bytecode carries lines.
fn upvalue_write(m: &Module, id: usize, upvalue: usize, depth: usize) -> Option<u32> {
    // Functions nest as the source does; the parser stops far earlier.
    if depth > 200 {
        return None;
    }
    let p = m.protos.get(id)?;
    let mut pc = 0;
    while pc < p.code.len() {
        let insn = p.code[pc];
        let op = insn & 0xff;
        if op == OP_SETUPVAL && ((insn >> 16) & 0xff) as usize == upvalue {
            return p.line(pc);
        }
        let d = (insn as i32 >> 16) as usize;
        let child = match op {
            OP_NEWCLOSURE => p.children.get(d).map(|&child| child as usize),
            OP_DUPCLOSURE => match p.constants.get(d) {
                Some(Constant::Closure(child)) => Some(*child as usize),
                _ => None,
            },
            _ => None,
        };
        pc += op_length(op);
        let Some(child) = child else { continue };
        let upvalues = m.protos.get(child)?.upvalues as usize;
        for k in 0..upvalues {
            let capture = *p.code.get(pc + k)?;
            let passed = capture & 0xff == OP_CAPTURE
                && (capture >> 8) & 0xff == LCT_UPVAL
                && ((capture >> 16) & 0xff) as usize == upvalue;
            if let Some(line) = passed
                .then(|| upvalue_write(m, child, k, depth + 1))
                .flatten()
            {
                return Some(line);
            }
        }
        pc += upvalues;
    }
    None
}

/// The global path of a GETIMPORT whose AUX word is `aux`, as `math.floor`.
fn import_path(p: &Proto, aux: u32) -> String {
    let count = (aux >> 30) as usize;
    let path: Vec<&str> = [aux >> 20, aux >> 10, aux]
        .iter()
        .take(count)
        .map(|&k| match p.constants.get((k & 1023) as usize) {
            Some(Constant::Text(Some(name))) => name.as_str(),
            _ => "?",
        })
        .collect();
    path.join(".")
}

/// What a register holds, as far as the walk for literals knows it.
#[derive(Clone, Debug, PartialEq)]
enum Held {
    Unknown,
    Text(String),
    /// A global path (`scene`, `scene.find`).
    Global(String),
    /// A value read as the field of another (`obj.attr`).
    Field(String),
    /// What a NAMECALL loads: the method's name and the field its receiver was read as.
    Method(String, Option<String>),
    /// A parameter of the function, its register written by no instruction since.
    Parameter(usize),
}

/// The instruction a jump at `pc` goes to; none for every other instruction and for the jumps
/// of fast calls, which only skip the CALL that follows them.
fn jump_target(pc: usize, insn: u32) -> Option<usize> {
    let offset = match insn & 0xff {
        // LOADB jumps by C when C is not 0.
        3 => match insn >> 24 {
            0 => return None,
            c => c as i64,
        },
        // JUMP to JUMPIFNOTLT, the loops' instructions, JUMPXEQK* and CMPPROTO jump by D.
        23..=32 | 56..=59 | 61 | 76..=80 | 88 => i64::from(insn as i32 >> 16),
        // JUMPX jumps by E.
        67 => i64::from(insn as i32 >> 8),
        _ => return None,
    };
    usize::try_from(pc as i64 + 1 + offset).ok()
}

/// The registers instruction `insn` writes, from its A on: none, A alone, A and A+1, or every
/// one from A (calls, varargs and loops, whose results reach the top); every one for an
/// instruction it does not know.
fn written(insn: u32) -> std::ops::Range<usize> {
    let a = ((insn >> 8) & 0xff) as usize;
    let end = match insn & 0xff {
        2..=7 | 9 | 12 | 13 | 15 | 17 | 19 | 33..=54 | 64 | 66 | 71 | 72 | 81..=83 | 90 => a + 1,
        20 | 85 => a + 2,
        21 | 56..=59 | 61 | 63 | 76 | 87 => 256,
        0
        | 1
        | 8
        | 10
        | 11
        | 14
        | 16
        | 18
        | 22..=32
        | 55
        | 60
        | 62
        | 65
        | 67..=70
        | 73..=75
        | 77..=80
        | 84
        | 86
        | 88
        | 89 => return 0..0,
        _ => return 0..256,
    };
    a..end.min(256)
}

/// Collects the literal texts of `p` at the sites of [`Site`], walking its code in order with
/// what each register holds. What reaches an instruction by a jump is not followed: at a jump's
/// target every register is unknown again, but the parameters no instruction writes. So a text
/// counts only when it is loaded on the way to its use, as the compiler loads arguments.
fn literals(p: &Proto, found: &mut Vec<Literal>) {
    // The jumps' targets and the registers some instruction writes, AUX words skipped.
    let mut targets = BTreeSet::new();
    let mut changed = [false; 256];
    let mut pc = 0;
    while pc < p.code.len() {
        targets.extend(jump_target(pc, p.code[pc]));
        changed[written(p.code[pc])].fill(true);
        pc += op_length(p.code[pc] & 0xff);
    }
    let parameters = usize::from(p.parameters);
    let start = |held: &mut [Held], all: bool| {
        for (r, slot) in held.iter_mut().enumerate() {
            *slot = if r < parameters && (all || !changed[r]) {
                Held::Parameter(r)
            } else {
                Held::Unknown
            };
        }
    };
    let text = |k: usize| match p.constants.get(k) {
        Some(Constant::Text(Some(text))) => Some(text.clone()),
        _ => None,
    };
    let mut held = vec![Held::Unknown; 256];
    start(&mut held, true);
    let mut pc = 0;
    while pc < p.code.len() {
        if targets.contains(&pc) {
            start(&mut held, false);
        }
        let insn = p.code[pc];
        let op = insn & 0xff;
        let (a, b, c) = (
            ((insn >> 8) & 0xff) as usize,
            ((insn >> 16) & 0xff) as usize,
            (insn >> 24) as usize,
        );
        let aux = p.code.get(pc + 1).copied().unwrap_or(0);
        let line = p.line(pc);
        let mut add = |site: Site, literal: &str| {
            found.push(Literal {
                site,
                text: literal.to_owned(),
                line,
            })
        };
        match op {
            OP_LOADK => {
                held[a] = text((insn as i32 >> 16) as usize).map_or(Held::Unknown, Held::Text)
            }
            OP_LOADKX => held[a] = text(aux as usize).map_or(Held::Unknown, Held::Text),
            OP_MOVE => held[a] = held[b].clone(),
            OP_GETGLOBAL => held[a] = text(aux as usize).map_or(Held::Unknown, Held::Global),
            OP_GETIMPORT => held[a] = Held::Global(import_path(p, aux)),
            OP_GETTABLEKS | OP_GETUDATAKS | OP_SETTABLEKS | OP_SETUDATAKS => {
                // The userdata forms keep the constant in the low 16 bits of AUX.
                let k = match op {
                    OP_GETTABLEKS | OP_SETTABLEKS => aux,
                    _ => aux & 0xffff,
                };
                let key = text(k as usize);
                if let (Held::Field(field), Some(key)) = (&held[b], &key) {
                    let field = field.clone();
                    add(Site::Key { field }, key);
                }
                if (op == OP_SETTABLEKS || op == OP_SETUDATAKS)
                    && let (Held::Field(field), Some(key), Held::Text(value)) =
                        (&held[b], &key, &held[a])
                {
                    let (field, key) = (field.clone(), key.clone());
                    add(Site::Set { field, key }, value);
                }
                if op == OP_GETTABLEKS || op == OP_GETUDATAKS {
                    held[a] = match (&held[b], key) {
                        (Held::Global(path), Some(key)) => Held::Global(format!("{path}.{key}")),
                        (_, Some(key)) => Held::Field(key),
                        (_, None) => Held::Unknown,
                    };
                }
            }
            OP_GETTABLE | OP_SETTABLE => {
                let key = match &held[c] {
                    Held::Text(key) => Some(key.clone()),
                    _ => None,
                };
                if let (Held::Field(field), Some(key)) = (&held[b], &key) {
                    let field = field.clone();
                    add(Site::Key { field }, key);
                }
                if op == OP_GETTABLE {
                    held[a] = key.map_or(Held::Unknown, Held::Field);
                }
            }
            OP_NAMECALL | OP_NAMECALLUDATA => {
                let k = if op == OP_NAMECALL { aux } else { aux & 0xffff };
                let receiver = held[b].clone();
                held[a] = match (&receiver, text(k as usize)) {
                    (Held::Global(_) | Held::Text(_), _) | (_, None) => Held::Unknown,
                    (Held::Field(field), Some(name)) => Held::Method(name, Some(field.clone())),
                    (_, Some(name)) => Held::Method(name, None),
                };
                if let Some(slot) = held.get_mut(a + 1) {
                    *slot = receiver;
                }
            }
            OP_CALL | OP_CALLFB => {
                // B is the arguments + 1, or 0 for all up to the top: the registers past the
                // last argument were left unknown by the call that filled the top.
                let count = if b == 0 { 255 - a } else { b - 1 };
                let arguments = held.iter().skip(a + 1).take(count);
                match &held[a] {
                    Held::Global(function) => {
                        for (arg, value) in arguments.enumerate() {
                            if let Held::Text(literal) = value {
                                let function = function.clone();
                                add(Site::Call { function, arg }, literal);
                            }
                        }
                    }
                    Held::Method(name, field) => {
                        for (arg, value) in arguments.skip(1).enumerate() {
                            if let Held::Text(literal) = value {
                                let (name, field) = (name.clone(), field.clone());
                                add(Site::Method { name, field, arg }, literal);
                            }
                        }
                    }
                    _ => {}
                }
                held[a..].fill(Held::Unknown);
            }
            OP_JUMPXEQKS | OP_JUMPIFEQ | OP_JUMPIFNOTEQ => {
                let compared = if op == OP_JUMPXEQKS {
                    text((aux & 0xff_ffff) as usize).map(|literal| (&held[a], literal))
                } else {
                    match (&held[a], &held[(aux & 0xff) as usize]) {
                        (Held::Text(literal), other) | (other, Held::Text(literal)) => {
                            Some((other, literal.clone()))
                        }
                        _ => None,
                    }
                };
                if let (Some((Held::Parameter(param), literal)), Some(function)) =
                    (compared, &p.name)
                {
                    let site = Site::Compare {
                        function: function.clone(),
                        param: *param,
                    };
                    add(site, &literal);
                }
            }
            _ => held[written(insn)].fill(Held::Unknown),
        }
        pc += op_length(op);
    }
}

/// The name of the top-level local behind `write`, read from the same source compiled with
/// debug level 2 (the same registers and instructions, plus the locals' names).
pub fn local_name(full: &Module, write: &TopLevelWrite) -> Option<String> {
    let main = &full.protos[full.main];
    main.locals
        .iter()
        .filter(|l| {
            l.register == write.register
                && (l.start_pc as usize) <= write.pc
                && write.pc < l.end_pc as usize
        })
        .find_map(|l| l.name.clone())
}
