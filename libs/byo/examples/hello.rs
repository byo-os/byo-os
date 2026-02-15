use byo::byo;

fn main() {
    byo! {
        +layer content order=0
        +view greeting class="flex items-center justify-center h-full" {
            +text label content="Hello, world!" class="text-4xl font-bold text-white"
        }
    };
}
