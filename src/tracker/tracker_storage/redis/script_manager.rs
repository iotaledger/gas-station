use once_cell::sync::Lazy;
use redis::Script;

const RESERVE_GAS_COINS_SCRIPT: &str = include_str!("lua_scripts/aggr_increment_sum.lua");

pub struct ScriptManager;

impl ScriptManager {
    pub fn increment_aggr_sum_script() -> &'static Script {
        static SCRIPT: Lazy<Script> = Lazy::new(|| Script::new(RESERVE_GAS_COINS_SCRIPT));
        Lazy::force(&SCRIPT)
    }
}
