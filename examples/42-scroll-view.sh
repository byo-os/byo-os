#!/bin/bash
# Scroll-view demo — scrollable containers with scrollbar chrome.
#
# This example must run through the orchestrator (not directly to the compositor)
# because it uses daemon-owned types (scroll-view, button) that need expansion
# by the controls daemon.
#
# Launch with:
#   cargo run -p byo-orchestrator -- examples/42-scroll-view.sh

# Generate items for the left panel (modern scrollbar)
v_items=""
for i in $(seq 1 30); do
  v_items="$v_items
    +view v-item-$i class=\"flex items-center px-4 py-3 rounded-lg bg-zinc-800\" {
      +view v-item-$i-dot class=\"w-3 h-3 rounded-full bg-blue-500 mr-3\"
      +text v-item-$i-label content=\"Item $i\" class=\"text-white\"
    }"
done

# Generate items for the right panel (classic scrollbar)
c_items=""
for i in $(seq 1 30); do
  c_items="$c_items
    +view c-item-$i class=\"flex items-center px-4 py-3 rounded-lg bg-zinc-800\" {
      +view c-item-$i-dot class=\"w-3 h-3 rounded-full bg-blue-500 mr-3\"
      +text c-item-$i-label content=\"Item $i\" class=\"text-white\"
    }"
done

# Generate colored blocks for horizontal scrolling
hblocks=""
colors=("bg-red-500" "bg-orange-500" "bg-amber-500" "bg-yellow-500" "bg-lime-500" "bg-green-500" "bg-emerald-500" "bg-teal-500" "bg-cyan-500" "bg-sky-500" "bg-blue-500" "bg-indigo-500" "bg-violet-500" "bg-purple-500" "bg-fuchsia-500" "bg-pink-500" "bg-rose-500" "bg-red-400" "bg-orange-400" "bg-amber-400")
for i in $(seq 1 20); do
  idx=$(( (i - 1) % ${#colors[@]} ))
  hblocks="$hblocks
    +view hblock-$i class=\"min-w-20 h-16 rounded-lg ${colors[$idx]} flex items-center justify-center\" {
      +text hblock-$i-label content=\"$i\" class=\"text-white font-bold\"
    }"
done

send() {
  printf '\e_B %s \e\\' "$1"
}

printf '\e_B
  +view root class="flex flex-col gap-6 p-8 w-full h-full bg-zinc-900" {

    +text title content="Scroll View Demo" class="text-2xl text-white font-bold"

    +view columns class="flex gap-6 flex-1 min-h-0" {

      +view left class="flex flex-col gap-3 flex-1 min-w-0 min-h-0" {
        +text v-title content="Vertical (modern)" class="text-lg text-zinc-400"
        +scroll-view vscroll direction=vertical class="flex-1 min-h-0" width=100%% {
          +view vscroll-content class="flex flex-col gap-2 p-2" {
            '"$v_items"'
          }
        }
        +view v-controls class="flex gap-2 mt-1" {
          +button v-top label="Scroll Top" events="press"
          +button v-down label="Scroll Down 100px" events="press"
          +button v-bottom label="Scroll Bottom" events="press"
        }
      }

      +view right class="flex flex-col gap-3 flex-1 min-w-0 min-h-0" {
        +text c-title content="Vertical (classic)" class="text-lg text-zinc-400"
        +scroll-view cscroll direction=vertical scrollbar=classic class="flex-1 min-h-0" width=100%% {
          +view cscroll-content class="flex flex-col gap-2 p-2" {
            '"$c_items"'
          }
        }
      }
    }

    +view h-section class="flex flex-col gap-3" {
      +text h-title content="Horizontal" class="text-lg text-zinc-400"
      +scroll-view hscroll direction=horizontal width=100%% height=80px {
        +view hscroll-content class="flex gap-2 p-2" {
          '"$hblocks"'
        }
      }
      +view h-controls class="flex gap-2 mt-1" {
        +button h-left label="Scroll Left" events="press"
        +button h-right label="Scroll Right 100px" events="press"
        +button h-end label="Scroll End" events="press"
      }
    }
  }
\e\\'

# Event loop — handle button presses for scroll commands
while IFS=$'\t' read -r OP KIND SEQ ID PROPS BODY; do

  case "$OP" in
    "!")
      if [ "$KIND" = "press" ]; then
        case "$ID" in
          v-top)    send ".scroll-to vscroll y=0" ;;
          v-down)   send ".scroll-by vscroll dy=100" ;;
          v-bottom) send ".scroll-to vscroll y=999999" ;;
          h-left)   send ".scroll-to hscroll x=0" ;;
          h-right)  send ".scroll-by hscroll dx=100" ;;
          h-end)    send ".scroll-to hscroll x=999999" ;;
        esac
        send "!ack press $SEQ handled"
      else
        send "!ack $KIND $SEQ"
      fi
      ;;
  esac

done < <(byo parse)
