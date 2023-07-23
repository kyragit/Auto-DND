# ACKS Automation Tool

## Features

- ~~Dice roller~~
- ~~Store and edit character sheets~~
- ~~Character generator~~
    - ~~Generates 5 blank characters, player must pick 2~~
- Combat simulation (no map)
    - ~~Automatically updates character sheets~~
    - Calculates XP gain
    - Auto morale rolls
    - Mortal wounds
    - Saving throws
- Time calculator
- ~~Possible integration with foundry vtt?~~ Impossible without way too much work
- Auto reaction rolls
- Inventory management
- ~~Auto level up~~
- ~~Sync server/client without jank~~
- ~~Proficiency viewer~~
- ~~Enemy viewer (for the dm)~~
- ~~Spell viewer~~
- Treasure generator
- Encounter generator
- ~~Hextml integration??~~ Impossible without way too much work
- ~~Dungeon scrawl integration????~~ Impossible without way too much work
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


## TODO

### Features
- Finish DM-side character sheets *
- Class damage bonus **
- Implement cleaves **
- Figure out spell repertoire and levelling **
- Create a "party" system. The party stores temporary XP to allocate. ***
    - Let players create parties
- Implement henchmen. **
- XP Calculation (stores xp gained, then DM can press button when in town) ***
- ~~Probably replace the idea of "deployed" enemies/items with a more general idea of "maps".~~ ***
    - They are not literal maps, but rather a list of "rooms" that have an ID and a list of all the
    items/enemies in that room. These maps are stored to disk and so are only loaded when they're 
    actually being used. They might also support random encounters and/or random treasure generation.
    Rooms could also have descriptions and a list of all connecting rooms, so that the DM can create
    levels completely without a (normal, spacial) map. 
    - ~~Maybe use `egui_cable` or `egui_node_graph` for a visual representation of how rooms connect?~~ Sadly, neither work for the time being.
    - Important question: should the maps store changeable state (like the health of each enemy) or
    not? If not, where will that state be stored? If so, will there be a way to reset the map to 
    its initial state? 
        - Arguments in favor of storing it:
            - Everything will be persistent between parties and play sessions. 
            - The entire map is just one file.
            - It'd probably be easier to understand.
        - Arguments against storing it:
            - In order to return the map to its initial state, you'd have to make a copy of it.
        - I'm gonna go with yes, it should.
- Treasure generator **
- Enemy descriptions *
- ~~Support multiple stored fights~~ ***
- ~~Connect the combat system with maps~~ ***
    - Each map stores an optional fight object. I think I will require fights to be inside of a map,
    because it makes it far more complicated if I don't, and the DM can just make a "dummy map" for
    fights out in the wilderness or whatever. This way, I also get having multiple stored fights 
    for free.
- ~~DM "tellraw" command~~ *

### Technical
- ~~Create a generic registry viewer~~ **
- Refactor everything... *sigh* *

### Visual
- Custom icon *
- Custom font/theme *
- Properly decide on a name **