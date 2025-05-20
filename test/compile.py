from pathlib import Path
import glob
from typing import Callable
import subprocess

CLANG = "clang"
CFLAGS = [
	"-O3"
]

def compileC(input: Path, output: Path):
	print(f"Compiling {input} to {output}")
	output.parent.mkdir(exist_ok=True, parents=True)
	r = subprocess.call([
		CLANG,
	] + CFLAGS + [
		"-emit-llvm",
		"-c",
		input,
		"-o",
		output
	])
	if r != 0:
		raise Exception(f"Failed to compile, status code {r}")

def main(mainDir: Path):
	out = mainDir / "out"
	calc_output: Callable[[Path], Path] = lambda x: (out / x.relative_to(mainDir).with_suffix(".bc"))

	# Compile all C programs
	cDir = (mainDir / "c")
	for cFile in glob.glob("**/*.c", root_dir=cDir, recursive=True):
		cFile = cDir / cFile
		compileC(cFile, calc_output(cFile))
	pass

if __name__ == "__main__":
	main(Path(__file__).parent.resolve())