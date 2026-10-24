//! The sandboxed Luau state a script runs in. Its only effects are the
//! catalog's steps: no clock, randomness, files, network, processes,
//! `require`, or code loading, so a script makes the same calls every time.

use crate::{Budget, Refusal};
use mlua::{Lua, LuaOptions, StdLib, VmState};
use std::{cell::Cell, rc::Rc};

/// Bounds on one script's own computation; step work is bounded by caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Luau's interrupt checks: one per loop iteration and call, roughly.
    pub interrupts: u64,
    pub memory: usize,
    /// Steps one run may call.
    pub steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            interrupts: 10_000_000,
            memory: 32 * 1024 * 1024,
            steps: 32,
        }
    }
}

/// Globals Luau's base library and mlua add that could break determinism,
/// load code, or catch a refusal and carry on.
const REMOVED: &[&str] = &[
    "collectgarbage",
    "gcinfo",
    "getfenv",
    "loadstring",
    "newproxy",
    "pcall",
    "print",
    "require",
    "setfenv",
    "xpcall",
];

/// Counts interrupt checks; reset before the load and before each run.
#[derive(Clone, Default)]
pub(crate) struct Meter(Rc<Cell<u64>>);

impl Meter {
    pub(crate) fn reset(&self) {
        self.0.set(0);
    }
}

/// A state with the safe libraries and the budget, not yet sandboxed: the
/// caller adds the host's globals, then calls [`seal`].
pub(crate) fn open(limits: Limits, meter: &Meter) -> mlua::Result<Lua> {
    let libs =
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::BIT | StdLib::BUFFER;
    let lua = Lua::new_with(libs, LuaOptions::new())?;
    let globals = lua.globals();
    for name in REMOVED {
        globals.raw_set(*name, mlua::Value::Nil)?;
    }
    let math = globals.raw_get::<mlua::Table>("math")?;
    math.raw_set("random", mlua::Value::Nil)?;
    math.raw_set("randomseed", mlua::Value::Nil)?;
    lua.set_memory_limit(limits.memory)?;
    let count = meter.0.clone();
    lua.set_interrupt(move |_| {
        let used = count.get().saturating_add(1);
        count.set(used);
        if used > limits.interrupts {
            return Err(mlua::Error::external(Refusal::Budget(Budget::Interrupts)));
        }
        Ok(VmState::Continue)
    });
    Ok(lua)
}

/// Freezes every library and the host's globals; the script's own globals
/// go to a fresh table on top.
pub(crate) fn seal(lua: &Lua) -> mlua::Result<()> {
    lua.sandbox(true)
}
