//! `cargo run -p gothic-ui --example ui_demo -- --ui-demo Hamlar --shots` (screens -> .local/ui-*.png)
fn main() -> anyhow::Result<()> {
    gothic_ui::demo::run(&std::env::args().collect::<Vec<_>>())
}
