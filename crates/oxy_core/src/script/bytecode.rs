//! A small reader of the bytecode the engine's fixed compiler emits (version 9, type information
//! version 3 in Luau 0.740), enough for the load checks: top-level locals that some function
//! writes (captures by reference in the main function), closures that share a mutable upvalue
//! with their creator, and the global paths a script imports. Any other version is refused, so
//! an mlua update that changes the format fails the contract test instead of passing unread.
use std::collections::BTreeSet;

/// The versions in the bytecode's first two bytes.
pub const VERSION: u8 = 9;
pub const TYPES_VERSION: u8 = 3;

// `LuauCaptureType` and the `LuauOpcode` numbers (Common/include/Luau/Bytecode.h).
const LCT_VAL: u32 = 0;
const LCT_REF: u32 = 1;
const LCT_UPVAL: u32 = 2;
const OP_SETUPVAL: u32 = 10;
const OP_GETIMPORT: u32 = 12;
const OP_NEWCLOSURE: u32 = 19;
const OP_DUPCLOSURE: u32 = 64;
const OP_CAPTURE: u32 = 70;
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
    upvalues: u8,
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
    let _parameters = r.u8()?;
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
    let _name = r.string()?;
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

/// What the load checks read from a chunk.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub top_level_writes: Vec<TopLevelWrite>,
    /// The global paths the chunk imports, as `math.floor`.
    pub imports: BTreeSet<String>,
    /// The functions (proto indexes, `lua_Debug.bytecodeid`) whose closures share a mutable
    /// upvalue with the function that creates them.
    pub stateful_protos: BTreeSet<u32>,
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
                    let count = (aux >> 30) as usize;
                    let path: Vec<String> = [aux >> 20, aux >> 10, aux]
                        .iter()
                        .take(count)
                        .map(|&k| match p.constants.get((k & 1023) as usize) {
                            Some(Constant::Text(Some(name))) => name.clone(),
                            _ => "?".into(),
                        })
                        .collect();
                    report.imports.insert(path.join("."));
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
