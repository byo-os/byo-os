#!/bin/bash
# Controls demo — buttons, checkboxes, and sliders via the controls daemon.
#
# This example must run through the orchestrator (not directly to the compositor)
# because it uses daemon-owned types (button, checkbox, slider) that need
# expansion by the controls daemon.
#
# Launch with:
#   cargo run -p byo-orchestrator -- examples/38-controls.sh

printf '\e_B
  +view root class="flex flex-col gap-6 p-8 w-full h-full bg-zinc-900" {

    +text title content="Controls Demo" class="text-2xl text-white font-bold"

    +view buttons class="flex flex-col gap-3" {
      +text btn-title content="Buttons" class="text-lg text-zinc-400"
      +view btn-row class="flex gap-3 items-center" {
        +button save label="Save" variant=primary events="press"
        +button cancel label="Cancel" events="press"
        +button delete label="Delete" variant=danger events="press"
        +button ghost label="Ghost" variant=ghost events="press"
        +button disabled label="Disabled" disabled
      }
    }

    +view checkboxes class="flex flex-col gap-3" {
      +text chk-title content="Checkboxes" class="text-lg text-zinc-400"
      +view chk-row class="flex gap-6 items-center" {
        +checkbox agree label="I agree to the terms"
        +checkbox newsletter label="Subscribe to newsletter" default-checked=true
        +checkbox disabled-chk label="Disabled" disabled
      }
    }

    +view sliders class="flex flex-col gap-3" {
      +text slider-title content="Sliders" class="text-lg text-zinc-400"
      +view slider-col class="flex flex-col gap-4 w-80" {
        +slider volume label="Volume" default-value=75
        +slider brightness label="Brightness" default-value=50
        +slider custom label="Custom Range" min=0 max=10 step=1 default-value=5
      }
    }
  }
\e\\'

# Read events from stdin and log them
while IFS= read -r line; do
  echo "$line" >&2
done
