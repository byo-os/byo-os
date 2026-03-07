#!/bin/bash
# Measure demo — request computed dimensions of UI elements.
#
# Click any button to send a `?measure` request for the corresponding view.
# The compositor responds with width, height, content-width, and content-height
# in logical pixels, displayed in a results panel.
#
# This example must run through the orchestrator (not directly to the compositor)
# because it uses daemon-owned types (button) that need expansion by the
# controls daemon, and `?measure` requests are routed through the orchestrator.
#
# Launch with:
#   cargo run -p byo-orchestrator -- examples/43-measure.sh

set -euo pipefail

send() {
  printf '\e_B %s \e\\' "$1"
}

# ---------------------------------------------------------------------------
# Build the UI — a variety of elements to measure
# ---------------------------------------------------------------------------

send '
  +view root class="flex flex-col gap-6 p-8 w-full h-full bg-zinc-900" {

    +text title content="Measure Demo" class="text-2xl text-white font-bold"
    +text subtitle content="Click a button to measure the element above it"
      class="text-sm text-zinc-500"

    +view targets class="flex gap-6 flex-wrap" {

      +view col-1 class="flex flex-col gap-3 items-center" {
        +view box-small class="w-24 h-24 rounded-xl bg-blue-500"
        +button measure-small label="Small Box" events="press"
      }

      +view col-2 class="flex flex-col gap-3 items-center" {
        +view box-wide class="w-64 h-16 rounded-xl bg-emerald-500"
        +button measure-wide label="Wide Box" events="press"
      }

      +view col-3 class="flex flex-col gap-3 items-center" {
        +view box-padded class="p-6 rounded-xl bg-amber-500" {
          +text box-padded-label content="Padded content"
            class="text-black font-bold"
        }
        +button measure-padded label="Padded Box" events="press"
      }

      +view col-4 class="flex flex-col gap-3 items-center" {
        +view box-auto class="rounded-xl bg-rose-500 p-4" {
          +text box-auto-label content="Auto-sized with a longer text label"
            class="text-white text-lg"
        }
        +button measure-auto label="Auto Box" events="press"
      }

      +view col-5 class="flex flex-col gap-3 items-center" {
        +view box-nested class="p-4 rounded-xl bg-violet-500 flex flex-col gap-2" {
          +view box-nested-a class="w-32 h-8 rounded bg-violet-300"
          +view box-nested-b class="w-24 h-8 rounded bg-violet-300"
          +view box-nested-c class="w-40 h-8 rounded bg-violet-300"
        }
        +button measure-nested label="Nested Box" events="press"
      }
    }

    +view results-header class="flex items-center gap-2" {
      +text results-title content="Measurements" class="text-lg text-zinc-400"
      +text result-line content="— click a button to measure" class="text-sm text-zinc-600"
    }
    +scroll-view results-scroll direction=vertical class="flex-1 min-h-0" width=100%% {
      +view results class="flex flex-col gap-2 p-2 w-full" {
        +text result-line content="Click a button to measure..." class="text-zinc-500"
      }
    }
  }
'

# ---------------------------------------------------------------------------
# Map button IDs to target view IDs
# ---------------------------------------------------------------------------

target_for() {
  case "$1" in
    measure-small)  echo "box-small" ;;
    measure-wide)   echo "box-wide" ;;
    measure-padded) echo "box-padded" ;;
    measure-auto)   echo "box-auto" ;;
    measure-nested) echo "box-nested" ;;
    *) echo "" ;;
  esac
}

# ---------------------------------------------------------------------------
# Event loop
# ---------------------------------------------------------------------------

MEASURE_SEQ=0
RESULT_COUNT=0

while IFS=$'\t' read -r OP KIND SEQ ID PROPS BODY; do

  case "$OP" in

    "!")
      # Button press → send ?measure for the corresponding view
      if [ "$KIND" = "press" ]; then
        TARGET=$(target_for "$ID")
        if [ -n "$TARGET" ]; then
          send "?measure $MEASURE_SEQ $TARGET"
          MEASURE_SEQ=$((MEASURE_SEQ + 1))
        fi
        send "!ack press $SEQ handled"
      else
        send "!ack $KIND $SEQ"
      fi
      ;;

    ".")
      # Response from ?measure
      if [ "$KIND" = "measure" ]; then
        W=$(echo "$PROPS" | byo prop width 2>/dev/null || echo "?")
        H=$(echo "$PROPS" | byo prop height 2>/dev/null || echo "?")
        CW=$(echo "$PROPS" | byo prop content-width 2>/dev/null || echo "?")
        CH=$(echo "$PROPS" | byo prop content-height 2>/dev/null || echo "?")

        RESULT_COUNT=$((RESULT_COUNT + 1))
        RID="result-${RESULT_COUNT}"

        # Remove placeholder on first result
        if [ "$RESULT_COUNT" -eq 1 ]; then
          send "-text result-line"
        fi

        send "@view results {
          +view $RID class=\"flex gap-3 px-3 py-2 rounded-lg bg-zinc-700\" {
            +text ${RID}-seq content=\"#$SEQ\" class=\"text-sm text-zinc-500 font-mono\"
            +text ${RID}-dims content=\"${W} × ${H}\" class=\"text-sm text-white font-mono\"
            +text ${RID}-content content=\"content: ${CW} × ${CH}\"
              class=\"text-sm text-zinc-400 font-mono\"
          }
        }"

        # Scroll to bottom (large value gets clamped to max)
        send "@scroll-view results-scroll scroll-y=999999"
      fi
      ;;

  esac

done < <(byo parse)
