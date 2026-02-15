use byo::byo;

struct FileEntry {
    name: &'static str,
    kind: &'static str,
    size: &'static str,
    modified: &'static str,
}

fn main() {
    let path = "/home/user/Documents";
    let files = vec![
        FileEntry {
            name: "Projects",
            kind: "folder",
            size: "-",
            modified: "Feb 12",
        },
        FileEntry {
            name: "Photos",
            kind: "folder",
            size: "-",
            modified: "Feb 10",
        },
        FileEntry {
            name: "resume.pdf",
            kind: "file",
            size: "142 KB",
            modified: "Jan 28",
        },
        FileEntry {
            name: "notes.txt",
            kind: "file",
            size: "4 KB",
            modified: "Feb 14",
        },
        FileEntry {
            name: "budget.csv",
            kind: "file",
            size: "12 KB",
            modified: "Feb 1",
        },
    ];
    let selected: Option<&str> = Some("notes.txt");

    byo! {
        +layer root order=0
        +view app class="flex flex-col h-full bg-zinc-900 text-zinc-200" {
            // Toolbar
            +view toolbar class="flex items-center gap-2 px-4 py-2 bg-zinc-800 border-b border-zinc-700" {
                +button back label="Back" variant="ghost"
                +button forward label="Forward" variant="ghost"
                +view breadcrumb class="flex-1 px-3" {
                    +text path content={path} class="text-sm text-zinc-400"
                }
                +view search class="w-64 bg-zinc-700 rounded-lg px-3 py-1" {
                    +text search_placeholder content="Search..." class="text-sm text-zinc-500"
                }
            }

            // Content
            +view content class="flex flex-1 overflow-hidden" {
                // Sidebar favorites
                +view sidebar class="w-48 bg-zinc-800/50 border-r border-zinc-700 py-2" {
                    +text fav_heading content="Favorites" class="px-4 py-1 text-xs font-semibold text-zinc-500 uppercase"
                    +view fav_home class="px-4 py-1 text-sm hover:bg-zinc-700 rounded mx-1 cursor-pointer" {
                        +text _ content="Home"
                    }
                    +view fav_docs class="px-4 py-1 text-sm bg-zinc-700 rounded mx-1" {
                        +text _ content="Documents"
                    }
                    +view fav_downloads class="px-4 py-1 text-sm hover:bg-zinc-700 rounded mx-1 cursor-pointer" {
                        +text _ content="Downloads"
                    }
                }

                // File list
                +view filelist class="flex-1 overflow-y-auto" {
                    +view header class="flex px-4 py-2 border-b border-zinc-700 text-xs font-semibold text-zinc-500 uppercase" {
                        +text col_name content="Name" class="flex-1"
                        +text col_size content="Size" class="w-24 text-right"
                        +text col_modified content="Modified" class="w-28 text-right"
                    }
                    for file in &files {
                        +view {format!("file-{}", file.name)} class={
                            if selected == Some(file.name) {
                                "flex items-center px-4 py-2 bg-blue-500/20 border border-blue-500/30 rounded mx-1"
                            } else {
                                "flex items-center px-4 py-2 hover:bg-zinc-800 rounded mx-1 cursor-pointer"
                            }
                        } {
                            +text {format!("icon-{}", file.name)} content={
                                if file.kind == "folder" { "\u{1F4C1}" } else { "\u{1F4C4}" }
                            } class="mr-3"
                            +text {format!("name-{}", file.name)} content={file.name} class="flex-1 text-sm"
                            +text {format!("size-{}", file.name)} content={file.size} class="w-24 text-right text-sm text-zinc-500"
                            +text {format!("mod-{}", file.name)} content={file.modified} class="w-28 text-right text-sm text-zinc-500"
                        }
                    }
                }
            }

            // Status bar
            +view statusbar class="flex items-center px-4 py-1 bg-zinc-800 border-t border-zinc-700 text-xs text-zinc-500" {
                +text item_count content={format!("{} items", files.len())}
                +view spacer class="flex-1"
                if let Some(name) = selected {
                    +text selected_info content={format!("Selected: {name}")}
                }
            }
        }
    };
}
