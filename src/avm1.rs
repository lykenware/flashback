use crate::timeline::Frame;
use avm1_parser::parse_cfg;
use avm1_types::cfg::{Action, Cfg, CfgBlock, CfgFlow};
use avm1_types::PushValue;

#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    I32(i32),
    F32(f32),
    F64(f64),
    Str(String),

    OpRes(usize),
}

impl Value {
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            Value::I32(x) => Some(x),
            Value::F32(x) if x == (x as i32 as f32) => Some(x as i32),
            Value::F64(x) if x == (x as i32 as f64) => Some(x as i32),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum Op {
    Play,
    Stop,
    GotoFrame(Frame),
    // FIXME(eddyb) can we statically resolve this?
    GotoLabel(String),
    GetUrl(String, String),

    GetVar(String),
    SetVar(String, Value),

    Call(Value, Vec<Value>),
    // FIXME(eddyb) integrate with GetMember.
    CallMethod(Value, String, Vec<Value>),
}

#[derive(Debug)]
pub struct Code {
    pub ops: Vec<Op>,
}

impl Code {
    pub fn parse_and_compile(data: &[u8]) -> Self {
        let cfg = parse_cfg(data);

        Code::compile(cfg)
    }

    pub fn compile(cfg: Cfg) -> Self {
        let mut consts = vec![];
        let mut regs = vec![];
        let mut stack = vec![];
        let mut ops = vec![];

        // HACK(eddyb) this hides the warnings / inference errors about `regs`.
        // FIXME(eddyb) remove after register writes are implemented.
        regs.push(Value::Undefined);
        regs.pop();

        // FIXME(demurgos) Handle control flow, we're currently only compiling the first block
        let block: CfgBlock = cfg.blocks.into_vec().remove(0);

        for action in block.actions {
            match action {
                Action::Play => ops.push(Op::Play),
                Action::Stop => ops.push(Op::Stop),
                Action::GotoFrame(goto) => {
                    ops.push(Op::GotoFrame(Frame(goto.frame as u16)));
                }
                Action::GotoLabel(goto) => {
                    ops.push(Op::GotoLabel(goto.label));
                }
                Action::GetUrl(get_url) => {
                    ops.push(Op::GetUrl(get_url.url, get_url.target));
                }
                Action::ConstantPool(pool) => {
                    consts = pool.pool;
                }
                Action::Push(push) => {
                    stack.extend(push.values.into_iter().map(|value| match value {
                        PushValue::Undefined => Value::Undefined,
                        PushValue::Null => Value::Null,
                        PushValue::Boolean(x) => Value::Bool(x),
                        PushValue::Sint32(x) => Value::I32(x),
                        PushValue::Float32(x) => Value::F32(x),
                        PushValue::Float64(x) => Value::F64(x),
                        PushValue::String(s) => Value::Str(s),

                        // FIXME(eddyb) avoid per-use cloning.
                        PushValue::Constant(i) => Value::Str(consts[i as usize].to_string()),
                        PushValue::Register(i) => regs[i as usize].clone(),
                    }));
                }
                Action::Pop => {
                    stack.pop();
                }
                Action::GetVariable => match stack.pop().unwrap() {
                    Value::Str(name) => {
                        ops.push(Op::GetVar(name));
                        stack.push(Value::OpRes(ops.len() - 1));
                    }
                    name => {
                        eprintln!("avm1: too dynamic GetVar({:?})", name);
                        break;
                    }
                },
                Action::SetVariable => {
                    let value = stack.pop().unwrap();
                    match stack.pop().unwrap() {
                        Value::Str(name) => {
                            ops.push(Op::SetVar(name, value));
                            stack.push(Value::OpRes(ops.len() - 1));
                        }
                        name => {
                            eprintln!("avm1: too dynamic SetVar({:?}, {:?})", name, value);
                            break;
                        }
                    }
                }
                Action::CallFunction => {
                    let name = stack.pop().unwrap();
                    let arg_count = stack.pop().unwrap();
                    match (name, arg_count.as_i32()) {
                        (Value::Str(name), Some(arg_count)) => {
                            let args = (0..arg_count).map(|_| stack.pop().unwrap()).collect();
                            ops.push(Op::GetVar(name));
                            ops.push(Op::Call(Value::OpRes(ops.len() - 1), args));
                            stack.push(Value::OpRes(ops.len() - 1));
                        }
                        (name, _) => {
                            eprintln!(
                                "avm1: too dynamic CallFunction({:?}, {:?})",
                                name, arg_count
                            );
                            break;
                        }
                    }
                }
                Action::CallMethod => {
                    let mut name = stack.pop().unwrap();
                    let this = stack.pop().unwrap();
                    let arg_count = stack.pop().unwrap();

                    if let Value::Str(s) = &name {
                        if s.is_empty() {
                            name = Value::Undefined;
                        }
                    }

                    match (name, arg_count.as_i32()) {
                        (Value::Undefined, Some(arg_count)) => {
                            let args = (0..arg_count).map(|_| stack.pop().unwrap()).collect();
                            ops.push(Op::Call(this, args));
                            stack.push(Value::OpRes(ops.len() - 1));
                        }
                        (Value::Str(name), Some(arg_count)) => {
                            let args = (0..arg_count).map(|_| stack.pop().unwrap()).collect();
                            ops.push(Op::CallMethod(this, name, args));
                            stack.push(Value::OpRes(ops.len() - 1));
                        }
                        (name, _) => {
                            eprintln!("avm1: too dynamic CallMethod({:?}, {:?})", name, arg_count);
                            break;
                        }
                    }
                }
                _ => {
                    eprintln!("unknown action: {:?}", action);
                    break;
                }
            }
        }

        match block.flow {
            // All of frames are loaded ahead of time, no waiting needed.
            CfgFlow::WaitForFrame(_) => {}
            CfgFlow::WaitForFrame2(_) => {
                stack.pop();
            }
            _ => {
                eprintln!("unknown flow: {:?}", block.flow);
            }
        }

        Code { ops }
    }
}
