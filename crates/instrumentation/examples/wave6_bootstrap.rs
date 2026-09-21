fn main() {
    print!("{}", localview_instrumentation::bootstrap_script(&Default::default()));
    print!("\n{}", localview_instrumentation::wave6::wave6_bootstrap_script());
}
