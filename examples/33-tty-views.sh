#!/bin/bash
# Example 33: Multiple TTY views — terminals inside styled containers
#
# Demonstrates mixing rich Tailwind-styled UI (borders, shadows, rounded
# corners, gradients, padding) with embedded terminal emulators.

# Build the layout
printf '\e_B
  +view root class="flex flex-col w-full h-full bg-zinc-950 p-6 gap-5" {

    // Title bar
    +view titlebar class="flex flex-row items-center gap-3 px-1" {
      +view dot-r class="w-3 h-3 rounded-full bg-red-500"
      +view dot-y class="w-3 h-3 rounded-full bg-yellow-500"
      +view dot-g class="w-3 h-3 rounded-full bg-green-500"
      +view spacer class="flex-1"
      +text title content="Terminal Dashboard" class="text-zinc-500 text-sm"
      +view spacer2 class="flex-1"
    }

    // Top row: three terminal panels
    +view top-row class="flex flex-row gap-5 flex-1" {

      // Left panel — blue theme with border
      +view panel-left class="flex flex-col flex-1 rounded-xl border border-blue-800 bg-blue-950 overflow-hidden" {
        +view header-left class="flex flex-row items-center gap-2 bg-blue-900/80 px-4 py-2 border-b border-blue-800 rounded-tl-xl rounded-tr-xl" {
          +view icon-left class="w-2 h-2 rounded-full bg-blue-400"
          +text label-left content="Stars" class="text-blue-300 text-sm"
        }
        +tty tty-left class="flex-1 text-sm"
      }

      // Center panel — green theme with border
      +view panel-center class="flex flex-col flex-1 rounded-xl border border-green-800 bg-green-950 overflow-hidden" {
        +view header-center class="flex flex-row items-center gap-2 bg-green-900/80 px-4 py-2 border-b border-green-800 rounded-tl-xl rounded-tr-xl" {
          +view icon-center class="w-2 h-2 rounded-full bg-green-400"
          +text label-center content="Dots" class="text-green-300 text-sm"
        }
        +tty tty-center class="flex-1 text-sm"
      }

      // Right panel — purple theme with border
      +view panel-right class="flex flex-col flex-1 rounded-xl border border-purple-800 bg-purple-950 overflow-hidden" {
        +view header-right class="flex flex-row items-center gap-2 bg-purple-900/80 px-4 py-2 border-b border-purple-800 rounded-tl-xl rounded-tr-xl" {
          +view icon-right class="w-2 h-2 rounded-full bg-purple-400"
          +text label-right content="System Info" class="text-purple-300 text-sm"
        }
        +tty tty-right class="flex-1 text-sm"
      }
    }

    // Bottom bar: log + status
    +view bottom-row class="flex flex-row gap-5 h-48" {

      // Log panel
      +view panel-log class="flex flex-col flex-1 rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden" {
        +view header-log class="flex flex-row items-center gap-2 bg-zinc-800/80 px-4 py-2 border-b border-zinc-700 rounded-tl-xl rounded-tr-xl" {
          +view icon-log class="w-2 h-2 rounded-full bg-amber-400"
          +text label-log content="Activity Log" class="text-zinc-400 text-sm"
        }
        +tty tty-log class="flex-1 text-sm"
      }

      // Status sidebar
      +view panel-status class="flex flex-col w-64 rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden" {
        +view header-status class="bg-zinc-800/80 px-4 py-2 border-b border-zinc-700 rounded-tl-xl rounded-tr-xl" {
          +text label-status content="Status" class="text-zinc-400 text-sm"
        }
        +view status-body class="flex flex-col gap-2 p-4" {
          +view row-stars class="flex flex-row items-center justify-between" {
            +text s1 content="Stars" class="text-blue-400 text-xs"
            +text s1v content="--" class="text-zinc-500 text-xs"
          }
          +view row-dots class="flex flex-row items-center justify-between" {
            +text s2 content="Dots" class="text-green-400 text-xs"
            +text s2v content="--" class="text-zinc-500 text-xs"
          }
          +view row-progress class="flex flex-col gap-1 mt-2" {
            +text s3 content="Progress" class="text-zinc-500 text-xs"
            +view bar-bg class="h-2 rounded-full bg-zinc-800" {
              +view bar-fill class="h-full w-0 rounded-full bg-indigo-500 transition-all duration-300"
            }
          }
        }
      }
    }
  }
\e\\'

# Helper: write to a specific tty
write_to() {
  local target="$1"
  shift
  printf '\e_B #redirect %s \e\\' "$target"
  printf "$@"
}

# Helper: patch a text node
patch() {
  printf '\e_B @text %s content="%s" \e\\' "$1" "$2"
}

# Helper: patch a view class
patch_view() {
  printf '\e_B @view %s class="%s" \e\\' "$1" "$2"
}

# Write system info to the right panel
write_to tty-right '\e[1;35m=== System Info ===\e[0m\n\n'
write_to tty-right '\e[36mHost:\e[0m    %s\n' "$(hostname)"
write_to tty-right '\e[36mUser:\e[0m    %s\n' "$(whoami)"
write_to tty-right '\e[36mDate:\e[0m    %s\n' "$(date)"
write_to tty-right '\e[36mShell:\e[0m   %s\n' "$SHELL"
write_to tty-right '\e[36mTerm:\e[0m    BYO/OS TTY\n'
write_to tty-right '\e[36mUptime:\e[0m %s\n' "$(uptime | sed 's/.*up//')"

# Log the start
write_to tty-log '\e[90m[%s]\e[0m Started multi-tty demo\n' "$(date +%H:%M:%S)"

total=200

# Animate stars and dots into the left and center panels
for i in $(seq 1 $total); do
  # Stars panel — random colored stars
  color=$(( (RANDOM % 6) + 31 ))
  write_to tty-left "\e[${color}m*\e[0m"

  # Dots panel — cycling colors
  dot_color=$(( (i % 6) + 31 ))
  write_to tty-center "\e[${dot_color}m.\e[0m"

  # Update status sidebar
  patch s1v "$i drawn"
  patch s2v "$i drawn"

  # Progress bar
  pct=$(( i * 100 / total ))
  patch_view bar-fill "h-full rounded-full bg-indigo-500 transition-all duration-300 w-[${pct}%]"

  # Periodic log messages
  if (( i % 25 == 0 )); then
    write_to tty-log '\e[90m[%s]\e[0m Wrote %d symbols (%d%%)\n' "$(date +%H:%M:%S)" "$i" "$pct"
  fi

  sleep 0.04
done

# Final updates
write_to tty-log '\e[90m[%s]\e[32m Animation complete!\e[0m\n' "$(date +%H:%M:%S)"
write_to tty-left '\n\n\e[33m%d stars drawn\e[0m\n' $total
write_to tty-center '\n\n\e[33m%d dots drawn\e[0m\n' $total

patch s1v "$total done"
patch s2v "$total done"
patch_view bar-fill "h-full rounded-full bg-green-500 transition-colors duration-500 w-full"
patch_view icon-left "w-2 h-2 rounded-full bg-green-400"
patch_view icon-center "w-2 h-2 rounded-full bg-green-400"

# Keep running
sleep 99999
