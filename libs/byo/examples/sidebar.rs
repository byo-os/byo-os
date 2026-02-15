use byo::byo;

fn main() {
    let items = [
        ("network", "Network"),
        ("display", "Display"),
        ("sound", "Sound"),
        ("storage", "Storage"),
        ("bluetooth", "Bluetooth"),
    ];

    byo! {
        +view sidebar class="w-64 h-full bg-zinc-800 py-2" {
            +text heading content="Settings" class="px-4 py-2 text-xs font-semibold text-zinc-400 uppercase tracking-wider"
            for (id, label) in &items {
                +view {format!("item-{id}")} class="px-4 py-2 text-sm text-zinc-200 hover:bg-zinc-700 cursor-pointer rounded-md mx-2" {
                    +text {format!("{id}-label")} content={*label}
                }
            }
        }
    };
}
