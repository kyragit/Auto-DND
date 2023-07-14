use std::collections::{HashMap, BTreeMap};

use serde::{Serialize, Deserialize};

use crate::{enemy::{EnemyType, Enemy}, item::Item};

/// A large data structure representing an entire map, like a dungeon or town. This is NOT a literal,
/// grid-based map; rather, it's just a collection of rooms without the notion of spatial position.
#[derive(Debug, Serialize, Deserialize)]
pub struct Map {
    /// The title of this map.
    pub name: String,
    /// A short description or overview of what's in this map.
    pub summary: String,
    // This is a BTreeMap so that iterating through the rooms yield a sensible order.
    /// All the rooms in this map. The key is conventionally the room number, but it could be anything really.
    pub rooms: BTreeMap<String, Room>,
    pub connections: HashMap<String, RoomConnection>,
}

impl Map {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            summary: String::new(),
            rooms: BTreeMap::new(),
            connections: HashMap::new(),
        }
    }
}

/// A room in a map.
#[derive(Debug, Serialize, Deserialize)]
pub struct Room {
    pub name: String,
    /// A description of the room, for the DM.
    pub description: String,
    /// All the enemies in this room.
    pub enemies: HashMap<String, (EnemyType, Vec<Enemy>)>,
    /// All the items in this room.
    pub items: RoomItems,
    /// All this room's connections. Key is the connection uuid, value is true if this is the 
    /// "from" side, false if it's the "to" side.
    pub connections: HashMap<String, bool>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            enemies: HashMap::new(),
            items: RoomItems::new(),
            connections: HashMap::new(),
        }
    }
}

/// A connection between two rooms.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoomConnection {
    pub from: String,
    pub to: String,
    pub one_way: bool,
    /// A description of this door/whatever.
    pub description: String,
    /// Whether this connection is currently passable or blocked.
    pub passable: bool,
    /// Whether the connection/door is locked.
    pub locked: bool,
    /// Stores the trap state, if any.
    pub trapped: Option<RoomTrap>,
}

impl RoomConnection {
    pub fn new() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            one_way: false,
            description: String::new(),
            passable: false,
            locked: false,
            trapped: None,
        }
    }
}

/// All of the items that are in this room.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoomItems {
    /// All of the items that are not in any particular container, whether that's on the floor or
    /// otherwise not contained by anything.
    pub loose_items: Vec<Item>,
    /// All of the containers in this room.
    pub containers: Vec<RoomContainer>,
}

impl RoomItems {
    pub fn new() -> Self {
        Self {
            loose_items: Vec::new(),
            containers: Vec::new(),
        }
    }
}

/// A container of some variety placed in a room. This could be a chest, bookcase, hidden compartment,
/// or even something like the surface of a table.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoomContainer {
    /// What this container should be called. Does not have to be unique.
    pub name: String,
    /// If trapped, stores what the trap does and whether it's been triggered.
    pub trapped: Option<RoomTrap>,
    /// Whether the container is locked or not.
    pub locked: bool,
    /// The different sections of the container, each holding items. This is so that items within
    /// the container can be seperated, like the different shelves of a bookcase or a hidden chamber
    /// within a chest.
    pub sections: HashMap<String, Vec<Item>>,
}

impl RoomContainer {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            trapped: None,
            locked: false,
            sections: HashMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RoomTrap {
    /// A description of what this trap does and how it works.
    pub description: String,
    /// Whether this trap is active.
    pub active: bool,
}

impl RoomTrap {
    pub fn new() -> Self {
        Self {
            description: String::new(),
            active: true,
        }
    }
}