#!/bin/bash
# Interactive pointer events demo — click buttons, see coordinates, hover effects
#
# Usage:
#   cargo run -p byo-orchestrator -- examples/31-pointer-events.sh

set -euo pipefail

# Helper: send BYO commands to orchestrator (stdout)
send() {
  printf '\e_B %s \e\\' "$1"
}

# ---------------------------------------------------------------------------
# Build the UI
# ---------------------------------------------------------------------------

send '
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-950 gap-8 p-8" {

    +text title content="Pointer Events Demo" class="text-3xl text-white"

    +view buttons class="flex flex-row gap-4" {
      +view btn-a class="px-6 py-3 rounded-xl bg-blue-500 transition duration-150"
        events="pointerdown, pointerup, pointerenter, pointerleave, click"
      {
        +text btn-a-label content="Button A" class="text-white text-lg"
      }
      +view btn-b class="px-6 py-3 rounded-xl bg-emerald-500 transition duration-150"
        events="pointerdown, pointerup, pointerenter, pointerleave, click"
      {
        +text btn-b-label content="Button B" class="text-white text-lg"
      }
      +view btn-c class="px-6 py-3 rounded-xl bg-rose-500 transition duration-150"
        events="pointerdown, pointerup, pointerenter, pointerleave, click verbose"
      {
        +text btn-c-label content="Button C (verbose)" class="text-white text-lg"
      }
    }

    +view tracker class="flex flex-col gap-2 p-6 rounded-2xl bg-zinc-900 border border-zinc-800 w-96"
      events="pointermove"
    {
      +text tracker-title content="Move Tracker" class="text-lg text-zinc-400"
      +text tracker-coords content="x: -- y: --" class="text-2xl text-white font-mono"
    }

    +view log-container class="flex flex-col gap-1 p-4 rounded-xl bg-zinc-900 border border-zinc-800 w-96 h-48 overflow-y-auto" {
      +text log-title content="Event Log" class="text-sm text-zinc-500"
    }
  }
'

# ---------------------------------------------------------------------------
# Button state machine — track hover and pressed independently
# ---------------------------------------------------------------------------

# State: 0=false, 1=true
HOVER_A=0; PRESS_A=0
HOVER_B=0; PRESS_B=0
HOVER_C=0; PRESS_C=0

# Compute visual state from hover + pressed flags
update_btn() {
  local id="$1" hover="$2" press="$3" base="$4" bright="$5" dark="$6"
  if [ "$press" = "1" ]; then
    send "@view $id class=\"px-6 py-3 rounded-xl $dark scale-95 transition duration-150\""
  elif [ "$hover" = "1" ]; then
    send "@view $id class=\"px-6 py-3 rounded-xl $bright scale-105 transition duration-150\""
  else
    send "@view $id class=\"px-6 py-3 rounded-xl $base transition duration-150\""
  fi
}

update_a() { update_btn btn-a "$HOVER_A" "$PRESS_A" bg-blue-500 bg-blue-400 bg-blue-700; }
update_b() { update_btn btn-b "$HOVER_B" "$PRESS_B" bg-emerald-500 bg-emerald-400 bg-emerald-700; }
update_c() { update_btn btn-c "$HOVER_C" "$PRESS_C" bg-rose-500 bg-rose-400 bg-rose-700; }

# ---------------------------------------------------------------------------
# Event loop: read events from stdin via byo parse, ACK them, update UI
# ---------------------------------------------------------------------------

LOG_COUNT=0
MAX_LOG=8

# The orchestrator sends APC-framed events on stdin.
# `byo parse` converts them to TSV records for easy shell processing.
while IFS=$'\t' read -r OP KIND SEQ ID PROPS BODY; do

  # Only process events (! prefix)
  [ "$OP" = "!" ] || continue

  case "$KIND" in
    pointermove)
      # Extract coordinates from the props blob via byo prop
      X=$(echo "$PROPS" | byo prop x 2>/dev/null || echo "--")
      Y=$(echo "$PROPS" | byo prop y 2>/dev/null || echo "--")
      send "@text tracker-coords content=\"x: ${X} y: ${Y}\""

      # ACK (not handled — let it bubble)
      send "!ack pointermove $SEQ"
      ;;

    pointerenter)
      case "$ID" in
        btn-a) HOVER_A=1; update_a ;;
        btn-b) HOVER_B=1; update_b ;;
        btn-c) HOVER_C=1; update_c ;;
      esac
      send "!ack pointerenter $SEQ"
      ;;

    pointerleave)
      case "$ID" in
        btn-a) HOVER_A=0; update_a ;;
        btn-b) HOVER_B=0; update_b ;;
        btn-c) HOVER_C=0; update_c ;;
      esac
      send "!ack pointerleave $SEQ"
      ;;

    pointerdown)
      case "$ID" in
        btn-a) PRESS_A=1; update_a ;;
        btn-b) PRESS_B=1; update_b ;;
        btn-c) PRESS_C=1; update_c ;;
      esac
      send "!ack pointerdown $SEQ"
      ;;

    pointerup)
      case "$ID" in
        btn-a) PRESS_A=0; update_a ;;
        btn-b) PRESS_B=0; update_b ;;
        btn-c) PRESS_C=0; update_c ;;
      esac
      send "!ack pointerup $SEQ"
      ;;

    click)
      # Log the click
      LOG_COUNT=$((LOG_COUNT + 1))
      LOG_ID="log-${LOG_COUNT}"

      # Remove oldest log entry if over limit
      if [ "$LOG_COUNT" -gt "$MAX_LOG" ]; then
        OLD_ID="log-$((LOG_COUNT - MAX_LOG))"
        send "-view $OLD_ID"
      fi

      send "@view log-container {
        +view $LOG_ID class=\"flex flex-row gap-2 px-2 py-1 rounded bg-zinc-800\" {
          +text ${LOG_ID}-kind content=\"click\" class=\"text-sm text-yellow-400 font-mono\"
          +text ${LOG_ID}-target content=\"$ID\" class=\"text-sm text-zinc-300 font-mono\"
          +text ${LOG_ID}-seq content=\"#$SEQ\" class=\"text-sm text-zinc-600 font-mono\"
        }
      }"

      # ACK handled (stop bubbling)
      send "!ack click $SEQ handled"
      ;;

    *)
      # ACK unknown events as not handled
      send "!ack $KIND $SEQ"
      ;;
  esac

done < <(byo parse)
