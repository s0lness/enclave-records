//! 256-word SAS list (one word per byte). Two-syllable, phonetically distinct
//! words in the spirit of the PGP word list. Two screens render it: the pairing
//! SAS, whose words are compared between two devices in the same room, and the
//! provenance head, whose words are compared against something a third party
//! wrote down. The second is why the list has a cross-language copy
//! (`tests/presse_client.py`, `SAS_WORDS`) and why the rendering rule is in
//! `docs/protocol.md`: an off-device verifier holding the 32-byte head must
//! produce the same words, or there is nothing to compare.

use alloc::format;
use alloc::string::String;

/// How many bytes of a chain head the device renders, and so how many words it
/// produces: eight, one per byte, 64 bits.
///
/// Sixty-four is the width that survives someone with a reason to lie. A forger
/// who fabricates a clone's history invents every key in it, so he can grind
/// links offline until his head agrees with the original's over whatever prefix
/// a screen shows. Thirty-two bits, the eight hex characters a Device ID uses,
/// falls to that in minutes; sixty-four does not. Words and not the sixteen
/// equivalent hex characters because words are what two humans actually
/// compare, which is why the pairing renders words rather than a digest, and
/// reading them aloud is a ritual this object already has.
pub const HEAD_WORDS: usize = 8;

/// The first [`HEAD_WORDS`] bytes of a chain head as words, two to a line.
///
/// Each of those bytes indexes [`WORDS`], in the order the head stores them,
/// and nothing else enters: two people comparing must derive the same words
/// from the same head, one of them off-device from `GET_BUNDLE p1=2`.
///
/// Two per line, and four lines, because the page's value column measures
/// 416px: the widest pair this list can produce stays well inside that (a
/// nineteen-character pair renders at 307), while three words pass it. A line
/// that wrapped would make the page's height depend on which head it happens to
/// show, which is the one thing a screen that does not scroll cannot afford.
pub fn head_words(head: &[u8; HEAD_WORDS]) -> String {
    let w = |i: usize| WORDS[head[i] as usize];
    format!(
        "{} {}\n{} {}\n{} {}\n{} {}",
        w(0),
        w(1),
        w(2),
        w(3),
        w(4),
        w(5),
        w(6),
        w(7)
    )
}

pub const WORDS: [&str; 256] = [
    "acrobat", "adrift", "almond", "amber", "anchor", "antique", "apple", "arena",
    "artist", "asteroid", "atlas", "autumn", "avocado", "azure", "bagpipe", "bamboo",
    "banjo", "barbecue", "beacon", "bedrock", "beehive", "bicycle", "billiard", "bison",
    "blizzard", "blossom", "bluebird", "bonfire", "bookshelf", "breeze", "brioche", "bronze",
    "bubble", "bucket", "butter", "cabaret", "cactus", "camera", "canyon", "caramel",
    "caravan", "carnival", "cascade", "castle", "cavern", "cello", "chapel", "checkers",
    "cherry", "chimney", "chorus", "cinema", "citrus", "clover", "cobalt", "coconut",
    "comet", "compass", "concert", "confetti", "copper", "coral", "cottage", "cricket",
    "crystal", "cyclone", "daisy", "dolphin", "domino", "dragon", "drizzle", "drumbeat",
    "dungeon", "eagle", "echo", "eclipse", "ember", "emerald", "engine", "falcon",
    "feather", "fiddle", "firefly", "flannel", "flamingo", "fortune", "fossil", "fountain",
    "freckle", "frontier", "galaxy", "garden", "gazelle", "geyser", "ginger", "glacier",
    "glitter", "goggles", "gondola", "granite", "grotto", "guitar", "hammock", "harbor",
    "harvest", "hazelnut", "helmet", "hexagon", "hickory", "horizon", "hummingbird", "iceberg",
    "igloo", "indigo", "island", "ivory", "jackal", "jasmine", "jigsaw", "jubilee",
    "juniper", "kayak", "kernel", "kettle", "kiwi", "lagoon", "lantern", "lavender",
    "lemonade", "leopard", "lighthouse", "lilac", "lobster", "locket", "lullaby", "magnet",
    "mango", "marble", "meadow", "melon", "meteor", "mineral", "mirror", "mosaic",
    "mountain", "muffin", "mustang", "nebula", "nectar", "noodle", "nugget", "nutmeg",
    "oasis", "obsidian", "octopus", "olive", "onyx", "opera", "orbit", "orchard",
    "organ", "otter", "oyster", "paddle", "pagoda", "panther", "papaya", "parade",
    "parasol", "peacock", "pebble", "pelican", "pencil", "penguin", "pepper", "petal",
    "phantom", "picnic", "pigeon", "pillow", "pinwheel", "pirate", "pistachio", "planet",
    "plaza", "polka", "pond", "poppy", "prairie", "pretzel", "prism", "pudding",
    "puffin", "pyramid", "quartz", "quiver", "raccoon", "radish", "rainbow", "raspberry",
    "raven", "reindeer", "ribbon", "riddle", "ripple", "rocket", "rooster", "rosemary",
    "ruby", "saffron", "sailboat", "sapphire", "sardine", "satchel", "seahorse", "shadow",
    "shamrock", "sherbet", "silver", "skylark", "sleigh", "snowflake", "sonnet", "sparrow",
    "sphinx", "spiral", "sprout", "squirrel", "stadium", "stallion", "starling", "sundial",
    "sunflower", "syrup", "tadpole", "tangerine", "tavern", "temple", "thimble", "thunder",
    "tiger", "timber", "toffee", "topaz", "trumpet", "tulip", "tundra", "turquoise",
    "twilight", "umbrella", "unicorn", "velvet", "violet", "volcano", "waffle", "walnut",
];
