#!/usr/bin/env python3
"""Builds the patina-desktop .deb (ADR-0099): this repository and the apps
packaging/bundle.toml pins, each built from a clean export of its commit,
laid out under /usr, into target/deb/.

    packaging/deb/build.py              this repository's last commit
    packaging/deb/build.py --worktree   its working tree, uncommitted changes
                                        and all, to try them; the version
                                        then says "dirty"
    packaging/deb/build.py --stage DIR --prefix /usr/local
                                        no .deb: the same tree laid out under
                                        DIR for that prefix, for make install-full
                                        (any OS, no dpkg needed)

Work goes in ~/.cache/patina-desktop/deb/: git caches of the apps (git/), their exports
(src/), and a cargo target folder each (target/), kept between builds so
rebuilds are incremental.
"""

import argparse
import os
import re
import shlex
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# Outside the repository: cargo would take an app exported under it for a
# member of this workspace.
WORK = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "patina-desktop/deb"
OUT = ROOT / "target/deb"
# Where the tree goes: /usr for the .deb, --prefix for make install-full.
PREFIX = "/usr"
BINDIR = "/usr/bin"
SHARE = "usr/share"
ICONDIR = "usr/share/icons/hicolor"


def say(message):
    print(f"==> {message}", flush=True)


def run(*command, cwd=None, capture=False):
    result = subprocess.run(command, cwd=cwd, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout.strip() if capture else None


def git(repo, *args):
    return run("git", "-C", str(repo), *args, capture=True)


def has_commit(repo, commit):
    return subprocess.run(["git", "-C", str(repo), "rev-parse", "--verify", "--quiet",
                           f"{commit}^{{commit}}"], stdout=subprocess.DEVNULL).returncode == 0


def fresh(name):
    """An empty export folder for `name`, whose target/ is its cargo target
    folder, kept between builds."""
    src, target = WORK / "src" / name, WORK / "target" / name
    shutil.rmtree(src, ignore_errors=True)
    src.mkdir(parents=True)
    target.mkdir(parents=True, exist_ok=True)
    (src / "target").symlink_to(target)
    return src


def export(repo, commit, name):
    """`commit` of `repo` into a fresh folder. git archive gives each file
    the commit's time, so cargo sees unchanged sources as unchanged."""
    src = fresh(name)
    run("sh", "-c", f"git -C {shlex.quote(str(repo))} archive {commit} | tar -x -C {shlex.quote(str(src))}")
    return src


def export_worktree(repo, name):
    """What git would commit: tracked files, and new ones it doesn't ignore."""
    src = fresh(name)
    run("sh", "-c", f"cd {shlex.quote(str(repo))} && git ls-files -z --cached --others --exclude-standard"
                    f" | tar -c --null -T - | tar -x -C {shlex.quote(str(src))}")
    return src


def checkout(app):
    """The app's pinned commit, fetched into a cache from its local checkout
    if that has it, else from its repository; returns the export and the
    full commit."""
    name, commit = app["name"], app["commit"]
    cache = WORK / "git" / f"{name}.git"
    if not cache.exists():
        run("git", "init", "--quiet", "--bare", str(cache))
    local = (ROOT / app.get("local", f"../{name}")).resolve()
    sources = [("local", str(local))] if (local / ".git").exists() else []
    for kind, source in sources + [("repo", app["repo"])]:
        if has_commit(cache, commit):
            break
        say(f"fetching {name} from {source}")
        run("git", "-C", str(cache), "fetch", "--quiet", source, f"+refs/heads/*:refs/{kind}/*")
    if not has_commit(cache, commit):
        sys.exit(f"error: {name}: commit {commit} isn't in {app['repo']}")
    full = git(cache, "rev-parse", f"{commit}^{{commit}}")
    return export(cache, full, name), full


def substitute(text):
    return text.replace("@BINDIR@", BINDIR)


def install_app(app, src, stage):
    """An app's binaries, desktop entry, icons and other files into stage."""
    name = app["name"]
    binaries = app.get("binaries", [name])
    for binary in binaries:
        shutil.copy2(src / "target/release" / binary, stage / BINDIR.lstrip("/") / binary)
    if app_id := app.get("id"):
        entry = (src / f"packaging/linux/{app_id}.desktop").read_text()
        for binary in binaries:
            entry = re.sub(rf"^Exec={re.escape(binary)}\b", f"Exec={BINDIR}/{binary}", entry, flags=re.M)
        put(stage / SHARE / f"applications/{app_id}.desktop", entry)
        icons = {src / f"packaging/linux/{name}.svg": f"scalable/apps/{app_id}.svg",
                 src / f"assets/{name}-icon-128.png": f"128x128/apps/{app_id}.png"}
        for icon, where in icons.items():
            if icon.exists():
                (stage / ICONDIR / where).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(icon, stage / ICONDIR / where)
    for where, source in app.get("files", {}).items():
        put(stage / where.lstrip("/"), substitute((src / source).read_text()))


def put(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def licenses(src, name, stage):
    for licence in sorted(src.glob("LICENSE*")) + sorted(src.glob("LICENCE*")):
        where = stage / SHARE / "doc/patina-desktop/licenses" / name / licence.name
        where.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(licence, where)


def is_elf(path):
    with open(path, "rb") as f:
        return f.read(4) == b"\x7fELF"


def shlib_depends(binaries):
    """The libraries the binaries link, as Depends, from dpkg-shlibdeps,
    which wants a debian/control to run beside."""
    folder = WORK / "shlibdeps"
    put(folder / "debian/control", "Source: patina-desktop\n\nPackage: patina-desktop\nArchitecture: any\n")
    out = run("dpkg-shlibdeps", "-O", "--warnings=0", *[f"-e{b}" for b in binaries],
              cwd=folder, capture=True)
    return out.split("=", 1)[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--worktree", action="store_true",
                        help="build this repository's working tree, not its last commit")
    parser.add_argument("--stage", metavar="DIR",
                        help="lay the tree out in DIR (under target/) instead of building a .deb")
    parser.add_argument("--prefix", default="/usr",
                        help="with --stage, where the tree will be installed (default /usr)")
    args = parser.parse_args()
    if args.prefix != "/usr" and not args.stage:
        sys.exit("error: --prefix is for --stage; the .deb installs into /usr")
    global PREFIX, BINDIR, SHARE, ICONDIR
    PREFIX = args.prefix.rstrip("/") or "/"
    BINDIR = f"{PREFIX}/bin"
    SHARE = f"{PREFIX.lstrip('/')}/share"
    ICONDIR = f"{SHARE}/icons/hicolor"

    head = git(ROOT, "rev-parse", "HEAD")
    if args.worktree:
        desktop = export_worktree(ROOT, "patina-desktop")
        dirty = bool(git(ROOT, "status", "--porcelain"))
    else:
        if git(ROOT, "status", "--porcelain"):
            say("patina-desktop has uncommitted changes; building its last commit (--worktree builds them)")
        desktop, dirty = export(ROOT, head, "patina-desktop"), False
    bundle = tomllib.loads((desktop / "packaging/bundle.toml").read_text())
    package = bundle["package"]

    if args.stage:
        stage = Path(args.stage).resolve()
        if not stage.is_relative_to(ROOT / "target"):
            sys.exit(f"error: --stage {stage} is emptied first, so it must be under {ROOT / 'target'}")
    else:
        arch = run("dpkg", "--print-architecture", capture=True)
        if arch != package["architecture"]:
            sys.exit(f"error: this machine builds {arch}, the bundle is {package['architecture']}")
        release = run("sh", "-c", ". /etc/os-release && echo $VERSION_CODENAME", capture=True)
        if release != package["distribution"]:
            say(f"this machine is {release}, the bundle is for {package['distribution']}: "
                "its libraries may not match")
        stage = WORK / "root"
    shutil.rmtree(stage, ignore_errors=True)
    (stage / BINDIR.lstrip("/")).mkdir(parents=True)

    built = []
    for app in bundle.get("app", []):
        src, commit = checkout(app)
        say(f"building {app['name']} {commit[:7]}")
        bins = [arg for binary in app.get("binaries", [app["name"]]) for arg in ("--bin", binary)]
        run("cargo", "build", "--release", "--locked", *bins, cwd=src)
        install_app(app, src, stage)
        licenses(src, app["name"], stage)
        built.append((app["name"], commit, app["repo"]))

    say(f"building patina-desktop {head[:7]}{'+dirty' if dirty else ''}")
    run("cargo", "build", "--release", "--locked", cwd=desktop)
    # make stage installs the launcher from where make build puts it.
    launcher = desktop / "target/launcher/bin"
    launcher.mkdir(parents=True, exist_ok=True)
    shutil.copy2(stage / BINDIR.lstrip("/") / "patina-launcher", launcher)
    layout = [f"PREFIX={PREFIX}"]
    if not args.stage:
        # The Makefile's own defaults are the host's, which is right for an install.
        layout += ["MANDIR=/usr/share/man", "SESSIONDIR=/usr/share/wayland-sessions",
                   "PAMDIR=/etc/pam.d", "XDGCONFDIR=/etc/xdg"]
    run("make", "--no-print-directory", "-C", str(desktop), "stage", f"DESTDIR={stage}", *layout)
    licenses(desktop, "patina-desktop", stage)
    built.insert(0, ("patina-desktop", head + ("+dirty" if dirty else ""), "this repository"))
    put(stage / SHARE / "patina-desktop/versions",
        "".join(f"{name} {commit} {repo}\n" for name, commit, repo in built))

    binaries = sorted(p for p in (stage / BINDIR.lstrip("/")).iterdir() if is_elf(p))
    run("strip", "--strip-unneeded", *map(str, binaries))
    if args.stage:
        say(f"staged {len(binaries)} binaries and the rest in {stage.relative_to(ROOT)}, for {PREFIX}")
        return

    say("packaging")
    for man in (stage / "usr/share/man").rglob("*.[0-9]"):
        run("gzip", "-9n", str(man))

    crate = tomllib.loads((desktop / "compositor/Cargo.toml").read_text())["package"]["version"]
    when = git(ROOT, "log", "-1", "--format=%cd", "--date=format-local:%Y%m%d%H%M", head)
    version = f"{crate}+{when}.g{head[:7]}" + ("+dirty" if dirty else "")

    depends = bundle.get("depends", {})
    replaces = ", ".join(depends.get("replaces", []))
    size = run("du", "-sk", "--apparent-size", str(stage), capture=True).split()[0]
    apps = "".join(f"  {app['name']}: {app['why']}\n" for app in bundle.get("app", []))
    control = {
        "Package": package["name"],
        "Version": version,
        "Architecture": package["architecture"],
        "Maintainer": package["maintainer"],
        "Installed-Size": size,
        "Depends": ", ".join([shlib_depends(binaries)] + depends.get("required", [])),
        "Recommends": ", ".join(depends.get("recommended", [])),
        "Provides": replaces,
        "Conflicts": replaces,
        "Replaces": replaces,
        "Section": "x11",
        "Priority": "optional",
        "Homepage": package["homepage"],
        "Description": "The Patina desktop, with its apps\n"
                       " A Wayland compositor with its bar, launcher, menus and lock screen,\n"
                       " and the apps it was tested with:\n"
                       f" .\n{apps}".rstrip("\n"),
    }
    debian = stage / "DEBIAN"
    put(debian / "control", "".join(f"{k}: {v}\n" for k, v in control.items() if v))
    # What's under /etc is configuration: an upgrade asks before replacing
    # a copy the admin changed.
    conffiles = sorted("/" + str(p.relative_to(stage)) for p in (stage / "etc").rglob("*") if p.is_file())
    put(debian / "conffiles", "".join(f"{p}\n" for p in conffiles))

    OUT.mkdir(parents=True, exist_ok=True)
    deb = OUT / f"{package['name']}_{version}_{package['architecture']}.deb"
    run("dpkg-deb", "--build", "--root-owner-group", "-Zzstd", str(stage), str(deb))
    say(f"built {deb.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"error: {shlex.join(map(str, e.cmd))} failed ({e.returncode})")
