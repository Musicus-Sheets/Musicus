#!/bin/sh
# Navigation simulation for a few example documents (names of the local test set; the PDFs
# are not in the repository). Usage: scripts/nav_examples.sh <pdf-dir> <out-dir>
set -eu
E=$1; O=$2; B=$(dirname "$0")/../target/release/nav-sim
$B "$E/303Stimme04_KlarinetteEb.pdf" --out "$O/clarinet-303" --render tab8-land:7:5 --render tab8-port:7:3 --render tab10.5-land:7:3 --render tab10.5-port:7:2 &
$B "$E/316Stimme07_KlarinetteEb.pdf" --out "$O/clarinet-316" --render tab8-land:7:3 &
$B "$E/01Stimme04_Klarinette2.pdf" --out "$O/clarinet-01" --render tab8-land:7:3 &
$B "$E/IMSLP633235-PMLP1016105-Dunhill_-_First_Year_Pieces.pdf" --pages 3-8 --out "$O/piano-dunhill" --render tab8-land:7:6 --render tab8-port:7:3 --render tab10.5-land:7:3 &
$B "$E/lake-pirates-notereader.pdf" --out "$O/piano-lake" --render tab8-land:7:4 &
$B "$E/america-lead-sheet-f.pdf" --out "$O/leadsheet-america" --render tab8-land:7:2 &
$B "$E/IMSLP180032-SIBLEY1802.7272.c9b6-39087017795040pp283-347.pdf" --pages 3-6 --out "$O/studies-arban" --render tab8-land:7:2 --render tab10.5-land:7:2 &
$B "$E/IMSLP455928-PMLP150102-bachNBAI,2gloriainexcelsisdeoBWV191.pdf" --pages 3-5 --out "$O/score-nba" --render tab8-land:7:2 &
wait
