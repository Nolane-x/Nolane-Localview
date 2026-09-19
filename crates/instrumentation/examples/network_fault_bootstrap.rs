use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

fn main() {
    print!("{}", bootstrap_script(&InstrumentationConfig::default()));
}
