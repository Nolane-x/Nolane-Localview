use localview_instrumentation::{InstrumentationConfig, bootstrap_script};

fn main() {
    print!("{}", bootstrap_script(&InstrumentationConfig::default()));
}
