ANT COLONY 1.0
==============

A small colony simulator for rusty486, in 640x480 with 256 colours. It is
written in R++ and built with Rustland R++; the source is C:\RPP\ANTS.RS.

HOW IT WORKS
  Workers leave the nest and wander. A worker that finds food carries a
  crumb home and leaves a violet scent trail, strongest near the food;
  other workers follow the scent, so busy trails grow between the nest
  and good food. Scent fades over time.

  The queen turns stored food into new ants (a worker costs 10, a soldier
  25) while Breed is on. Every ant eats: if the store runs dry the colony
  starves. Ants also grow old. Spiders arrive from the edges from day 2
  and eat ants; soldiers patrol the nest and fight them. New food grows
  now and then. If every ant dies, the colony is lost.

MOUSE
  Left button: use the tool (hold and drag for food, rock, water, erase).
  Right button: erase.
  Click the buttons in the panel.

TOOLS AND KEYS
  1 Food     drop food             W  breed a worker (10 food)
  2 Rock     build rock walls      S  breed a soldier (25 food)
  3 Water    make a pond           A  automatic breeding on/off
  4 Erase    back to grass         P  pause (or Space)
  5 Spider   release a spider      F  speed 1x, 2x, 4x
                                   N  new world      Esc quit

It runs at 33 MHz; with hundreds of ants a faster clock (Ctrl+F12,
Emulation) keeps it smooth.
