with open('core/src/lib.rs', 'r') as f:
    text = f.read()

text += "\\npub mod git_side_effects;\\n"
with open('core/src/lib.rs', 'w') as f:
    f.write(text)
