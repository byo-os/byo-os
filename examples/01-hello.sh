#!/bin/bash
# Hello World — basic view + text
printf '\e_B
  +view root class="flex items-center justify-center w-full h-full bg-zinc-900" {
    +text greeting content="Hello, BYO/OS!" class="text-2xl text-white"
  }
\e\\'

sleep 99999
