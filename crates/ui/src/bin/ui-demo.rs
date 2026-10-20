fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    gothic_ui::demo::run(&args)
}
