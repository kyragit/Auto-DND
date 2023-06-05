# ACKS Automation Tool

## Features

- Dice roller
- Store and edit character sheets
- Character generator
    - Generates 5 blank characters, player must pick 2
- Combat simulation (no map)
    - Automatically updates character sheets
    - Calculates XP gain
    - Auto morale rolls
    - Mortal wounds
    - Saving throws
- Time calculator
- Possible integration with foundry vtt?
- Auto reaction rolls
- Inventory management
- Auto level up
- Sync server/client without jank
- Proficiency viewer
- Enemy viewer (for the dm)
- Spell viewer
- Treasure generator
- Encounter generator
- Hextml integration??
- Dungeon scrawl integration????
- Campaign features
    - Magic research
    - Domain play
    - Ritual spells

## Guidelines

- Let players roll:
    - Don't automate to the point of absurdity, especially on the player's side. Players should not
    feel like the program is doing everything for them. In general, if a player is meant to do a 
    roll, have *them* click a button. Players should feel like they have agency!
    - At the same time, *do* automate tedious or boring tasks. No one likes doing inventory management
    for 20 minutes; let the computer do that!
- Flexibility:
    - Automation is great, but you can't account for every single niche case. Give the option to do
    some things manually, especially for the DM. 
- DM Control:
    - The DM should be able to do *anything*. Always give an option to override standard procedure.
- Networking:
    - Under most circumstances, the client should not do the hard work. Have the client ask the
    server to do something, and return the result. The client should get pretty much all of its
    information from the server, as to avoid desync.
- UI:
    - Don't make the UI a nightmare. Different panels should do different things, and it should be
    clear where a particular button is without having to check.