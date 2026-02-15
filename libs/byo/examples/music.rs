use byo::byo;

struct Track {
    id: &'static str,
    title: &'static str,
    artist: &'static str,
    duration: &'static str,
}

fn main() {
    let tracks = vec![
        Track {
            id: "1",
            title: "Starlight",
            artist: "Muse",
            duration: "3:58",
        },
        Track {
            id: "2",
            title: "Hyperballad",
            artist: "Bjork",
            duration: "5:21",
        },
        Track {
            id: "3",
            title: "Everything In Its Right Place",
            artist: "Radiohead",
            duration: "4:11",
        },
        Track {
            id: "4",
            title: "Midnight City",
            artist: "M83",
            duration: "4:03",
        },
    ];
    let now_playing = "2";

    byo! {
        +layer root order=0
        +view player class="flex flex-col h-full bg-zinc-900" {
            // Track list
            +view tracklist class="flex-1 overflow-y-auto" {
                +view header class="flex px-4 py-2 border-b border-zinc-700" {
                    +text col_title content="Title" class="flex-1 text-xs font-semibold text-zinc-500 uppercase"
                    +text col_artist content="Artist" class="w-48 text-xs font-semibold text-zinc-500 uppercase"
                    +text col_duration content="Duration" class="w-20 text-right text-xs font-semibold text-zinc-500 uppercase"
                }
                for track in &tracks {
                    +view {format!("track-{}", track.id)} class={
                        if track.id == now_playing {
                            "flex items-center px-4 py-3 bg-zinc-700/50 cursor-pointer"
                        } else {
                            "flex items-center px-4 py-3 hover:bg-zinc-800 cursor-pointer"
                        }
                    } {
                        +text {format!("title-{}", track.id)} content={track.title} class={
                            if track.id == now_playing {
                                "flex-1 text-sm text-green-400 font-medium"
                            } else {
                                "flex-1 text-sm text-zinc-200"
                            }
                        }
                        +text {format!("artist-{}", track.id)} content={track.artist} class="w-48 text-sm text-zinc-400"
                        +text {format!("dur-{}", track.id)} content={track.duration} class="w-20 text-right text-sm text-zinc-500"
                    }
                }
            }

            // Now playing bar
            +view nowplaying class="flex items-center gap-4 px-6 py-4 bg-zinc-800 border-t border-zinc-700" {
                +view art class="w-12 h-12 rounded bg-zinc-600"
                +view info class="flex-1" {
                    +text np_title content="Hyperballad" class="text-sm font-medium text-white"
                    +text np_artist content="Bjork" class="text-xs text-zinc-400"
                }
                +view controls class="flex items-center gap-3" {
                    +button prev label="Prev" variant="ghost"
                    +button play label="Pause" variant="primary"
                    +button next label="Next" variant="ghost"
                }
            }
        }
    };
}
