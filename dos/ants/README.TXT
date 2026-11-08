ANT COLONY 2.0
==============

An ant farm for rusty486: a side view of the colony, with sky and a
scrolling surface where food lies, and the earth below where the ants dig
their nest. 640x480 in 256 colours. Written in R++ with Rustland R++; the
source is C:\RPP\ANTS.RS.

HOW IT WORKS
  The world is three screens wide. The nest starts as a shaft down to the
  queen's chamber. Workers climb out, wander the surface and carry food
  home; a worker with food leaves a violet scent trail on the way back,
  and others follow it out to the food. Scent fades over time.

  Mark earth with the Dig tool and workers dig it out a cell at a time,
  carrying the soil up to heap it in mounds beside the entrance. Rock
  can't be dug. Ants walk along tunnels, over the ground and up slopes.

  The queen turns stored food into eggs and new ants (a worker costs 10,
  a soldier 25) while Breed is on; you can see the stores and eggs in her
  chamber. Every ant eats, and ants grow old. Spiders arrive along the
  surface from day 2 and hunt ants above ground - tunnels are safe.
  Soldiers stand guard near the entrance and fight them.

MOUSE
  Left button: use the tool (drag to dig or fill a strip).
  Right button: remove dig marks.
  Panel buttons; click the map (bottom right) to jump there.
  Move the pointer to the left or right edge to scroll.

TOOLS AND KEYS
  1 Dig      mark earth to dig     W  breed a worker (10 food)
  2 Food     drop food             S  breed a soldier (25 food)
  3 Fill     fill a tunnel in      A  automatic breeding on/off
  4 Spider   release a spider      P  pause (or Space)
  Left/Right scroll, Home: nest    F  speed 1x, 2x, 4x
                                   N  new world      Esc quit

It runs at 33 MHz; with hundreds of ants a faster clock (Ctrl+F12,
Emulation) keeps it smooth.
