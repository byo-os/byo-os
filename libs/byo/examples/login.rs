use byo::byo;

fn main() {
    let error: Option<&str> = Some("Invalid username or password");
    let loading = false;

    byo! {
        +layer root order=0
        +view container class="flex items-center justify-center h-full bg-zinc-900" {
            +view card class="w-96 bg-zinc-800 rounded-2xl shadow-xl p-8" {
                +text title content="Sign In" class="text-2xl font-bold text-white mb-6"

                if let Some(msg) = error {
                    +view alert class="bg-red-500/10 border border-red-500/20 rounded-lg p-3 mb-4" {
                        +text alert_msg content={msg} class="text-sm text-red-400"
                    }
                }

                +view fields class="flex flex-col gap-4 mb-6" {
                    +view username_field class="flex flex-col gap-1" {
                        +text username_label content="Username" class="text-sm text-zinc-400"
                        +view username_input class="bg-zinc-700 rounded-lg px-4 py-2 text-white"
                    }
                    +view password_field class="flex flex-col gap-1" {
                        +text password_label content="Password" class="text-sm text-zinc-400"
                        +view password_input class="bg-zinc-700 rounded-lg px-4 py-2 text-white"
                    }
                }

                +view actions class="flex flex-col gap-3" {
                    if loading {
                        +button submit label="Signing in..." disabled
                    } else {
                        +button submit label="Sign In" variant="primary"
                    }
                    +button forgot label="Forgot password?" variant="link"
                }
            }
        }
    };
}
