import os, pty, sys, time, random, select, fcntl, termios, struct, re

COLS, ROWS = 100, 34
# Records the demo in docs/media/demo.cast: vi on the manifest with syntax highlighting, :q, then the build.
#   python3 docs/media/record-demo.py OUT.cast [PROJECT_DIR] [PKGSMITH_BINARY]
# It needs asciinema (uv tool run --from asciinema asciinema), Podman, and a vim with +syntax. The vi of vim-minimal
# has no syntax support, so unpack vim-enhanced and vim-common (dnf download, rpm2cpio | cpio -idm) into a folder and
# set PKGSMITH_DEMO_VIM to that folder. Render the cast to a GIF with
#   agg --fps-cap 12 --idle-time-limit 1.2 --theme dracula --font-size 16 demo.cast demo.gif
cast = sys.argv[1]
project = sys.argv[2] if len(sys.argv) > 2 else "examples/demo"
binary = sys.argv[3] if len(sys.argv) > 3 else "target/release/pkgsmith"
import tempfile

home = tempfile.mkdtemp(prefix="pkgsmith-demo-")
os.makedirs(home + "/bin")
os.symlink(os.path.abspath(binary), home + "/bin/pkgsmith")
vim_root = os.environ.get("PKGSMITH_DEMO_VIM", "")
if vim_root:
    vim_runtime = (
        f"{vim_root}/usr/share/vim/"
        + sorted(os.listdir(f"{vim_root}/usr/share/vim"))[-1]
    )
    vi = home + "/bin/vi"
    open(vi, "w").write(
        f'#!/bin/sh\nexport VIM={vim_root}/usr/share/vim VIMRUNTIME={vim_runtime}\nexec {vim_root}/usr/bin/vim "$@"\n'
    )
    os.chmod(vi, 0o755)
open(home + "/rc", "w").write("PS1='\\[\\e[1;32m\\]$\\[\\e[0m\\] '\n")
env = dict(os.environ)
env.update(
    PS1="\\[\\e[1;32m\\]$\\[\\e[0m\\] ",
    PATH=home + "/bin:" + env["PATH"],
    TERM="xterm-256color",
    VIMINIT="syntax on | set number | set background=dark | set noswapfile | set shortmess+=I | set laststatus=2"
    " | set ruler | set t_RV= | set t_u7= | set t_RB= | set t_RF=",
    XDG_RUNTIME_DIR=env.get("XDG_RUNTIME_DIR", ""),
)
cmd = [
    "uv",
    "tool",
    "run",
    "--from",
    "asciinema",
    "asciinema",
    "rec",
    "--overwrite",
    "-q",
    "--cols",
    str(COLS),
    "--rows",
    str(ROWS),
    "-c",
    f"bash --noprofile --rcfile {home}/rc -i",
    cast,
]
pid, fd = pty.fork()
if pid == 0:
    os.chdir(project)
    os.execvpe(cmd[0], cmd, env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

buffer = ""


def pump(seconds):
    """Read output for a while, so the shell and the program never block on a full pipe."""
    global buffer
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                return
            buffer += data.decode("utf8", "replace")


def type_text(text, speed=0.055):
    for ch in text:
        os.write(fd, ch.encode())
        pump(speed + random.random() * 0.05)


def wait_for(pattern, timeout):
    end = time.time() + timeout
    while time.time() < end:
        pump(0.2)
        if re.search(pattern, buffer):
            return True
    return False


random.seed(7)
pump(2.5)  # the shell starts
type_text("vi pkgsmith.star")
os.write(fd, b"\r")
pump(3.5)  # the editor shows the manifest
for _ in range(3):  # scroll down a little, as if reading
    os.write(fd, b"\x04")
    pump(1.0)  # Ctrl-D, half a page
os.write(fd, b"gg")
pump(1.5)
type_text(":q", 0.12)
os.write(fd, b"\r")
pump(1.5)
type_text("pkgsmith plan")
os.write(fd, b"\r")
pump(2.0)
type_text("pkgsmith build -j 4 --no-cache")
os.write(fd, b"\r")
done = wait_for(r"Packages in", 180)
pump(3.0)
type_text("ls dist")
os.write(fd, b"\r")
pump(2.5)
type_text("exit")
os.write(fd, b"\r")
pump(1.5)
print("build finished:", done)
try:
    os.waitpid(pid, 0)
except ChildProcessError:
    pass
