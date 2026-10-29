"""The session starts and stays up."""

# The compositor by its path; the bar and wallpaper it starts by name, from its own folder.
PIECES = ["release/patina-compositor", "patina-bar", "patina-wallpaper"]


def test_the_sessions_pieces_are_running(vm):
    for piece in PIECES:
        assert vm.alive(piece), f"{piece} isn't running"


def test_nothing_in_the_session_has_panicked(vm):
    panics = vm.ssh("grep -ci panicked /tmp/patina-session.log", check=False)
    assert panics in ("", "0"), vm.ssh("grep -i -B2 -A5 panicked /tmp/patina-session.log")


def test_the_screen_is_drawn(vm):
    # The desktop isn't one flat colour: wallpaper, bar, a window.
    screen = vm.shot()
    colours = screen.getcolors(maxcolors=1_000_000)
    assert colours is not None and len(colours) > 20
