#!/bin/bash
# Run every QA scenario with the port-coverage build; one dump per scenario in .local/portcov/runs/.
# usage: scripts/portcov/run_all.sh <exe> [scenario ...]
EXE=${1:-.local/target-cov/release/gothic-reforged.exe}; shift
SCEN=${@:-"startup_sanity menu_save_load trade_click trade_esc loot_chest craft_anvil dialog_quest ui_screens combat_basic souls_basic souls_er magic_bow controls_basic bow_pose shaman_duel hud_focus swim routine_reentry lockpick sit_pose quick_slots teach doors sector_switch teleport walk_to_reddock world_walk triggers perf_idle"}
mkdir -p .local/portcov/runs
for s in $SCEN; do
  GOTHIC_PORTCOV_OUT=.local/portcov/runs/$s.json timeout 480 "$EXE" --play --qa "$s" > ".local/portcov/runs/$s.log" 2>&1
  echo "$s exit $?  $(grep -a 'QA RESULT' .local/portcov/runs/$s.log | tail -1)" >> .local/portcov/runs/_summary.txt
done
echo ALLDONE >> .local/portcov/runs/_summary.txt
