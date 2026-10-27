"""The fixed prompts of tests 1, 2 and 4.

Test 1 and test 4 share the 20 edit prompts: 13 on the island, 7 on an empty scene. Test 2's
10 placement prompts all ask for the same 2x2x2 magenta marker, so `score.py` can find it and
check where it landed against the island's own voxels.
"""

EDITS = [
    ("i01-roof-blue", "island", "Make the roof of the house dark blue."),
    ("i02-chimney-flag", "island", "Put a flagpole with a red flag on top of the chimney."),
    ("i03-fence", "island", "Build a low wooden fence around the house, with a gap where the path leaves the door."),
    ("i04-no-chimney", "island", "Remove the chimney."),
    ("i05-sea", "island", "Make the sea a turquoise tropical colour, keeping the shallow and the deep water different shades."),
    ("i06-boat", "island", "Add a small wooden rowing boat floating in the water next to the end of the dock."),
    ("i07-trees", "island", "Plant three more round leafy trees on open grass to the left of the house."),
    ("i08-flowers", "island", "Make every flower yellow."),
    ("i09-well", "island", "Add a stone well with a little roof on the grass in front of the house, beside the path."),
    ("i10-autumn", "island", "Give the pine trees autumn colours: orange and brown needles instead of green."),
    ("i11-lighthouse", "island", "Build a small lighthouse with red and white stripes at the far end of the dock."),
    ("i12-lamp", "island", "Add a lamp post beside the door of the house."),
    ("i13-bench", "island", "Put a wooden bench beside the path, facing it."),
    ("e01-pagoda", "empty", "Build a voxel pagoda garden: a three-tier pagoda with curved red roofs, a small pond, stepping stones and two small trees, on a grass base."),
    ("e02-tower", "empty", "Build a medieval stone tower about 24 voxels tall, with battlements on top and a wooden door."),
    ("e03-snowman", "empty", "Build a snowman with a carrot nose, coal eyes and a top hat."),
    ("e04-fence", "empty", "Build a square wooden picket fence around a 20 by 20 grass yard, with a gate on the front side."),
    ("e05-sailboat", "empty", "Build a small sailboat with a white sail, floating on a patch of blue water."),
    ("e06-mushroom", "empty", "Build a red mushroom house with white spots, a door and a round window."),
    ("e07-palm", "empty", "Build a palm tree on a small sand island surrounded by water."),
]

MARKER = "Place a marker: a 2x2x2 cube of a new magenta colour (#ff00ff)"

PLACEMENTS = [
    ("p01-chimney", f"{MARKER}, sitting on top of the chimney. Change nothing else."),
    ("p02-tallest-tree", f"{MARKER}, sitting on top of the tallest tree. Change nothing else."),
    ("p03-dock-end", f"{MARKER}, standing on the planks at the far end of the dock. Change nothing else."),
    ("p04-door", f"{MARKER}, on the ground just in front of the house's door. Change nothing else."),
    ("p05-ridge", f"{MARKER}, sitting on the roof ridge in the middle of the roof. Change nothing else."),
    ("p06-left-of-house", f"{MARKER}, on the ground to the left of the house, touching its wall. Change nothing else."),
    ("p07-behind-house", f"{MARKER}, on the ground behind the house, touching its back wall. Change nothing else."),
    ("p08-off-dock", f"{MARKER}, floating on the water just past the far end of the dock. Change nothing else."),
    ("p09-by-pine", f"{MARKER}, on the ground just to the right of the tallest pine tree. Change nothing else."),
    ("p10-sea-front", f"{MARKER}, floating on the open sea in front of the island, in line with the house. Change nothing else."),
]
