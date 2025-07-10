from pathlib import Path
import glob
from typing import Callable
import subprocess
import os
import shutil

mainDir = Path(__file__).parent.resolve()

CLANG = "clang"
CFLAGS = [
	"-O3"
]
CARGO = "cargo"
CODEGEN_BACKEND = mainDir / "../target/debug/librustc_codegen_jvlm.so"

def compileC(input: Path, output: Path):
	print(f"C: Compiling {input} to {output}")
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

def compileRust(input: Path):
	print(f"Rust: Compiling {input}")
	r = subprocess.call([
		shutil.which(CARGO),
		"build",
		"--release"
	], env={
		"CARGO_ENCODED_RUSTFLAGS": f"-Zcodegen-backend={CODEGEN_BACKEND}"
	}, cwd=input)
	if r != 0:
		raise Exception(f"Failed to compile, status code {r}")

def main():
	out = mainDir / "out"
	calc_output: Callable[[Path], Path] = lambda x: (out / x.relative_to(mainDir).with_suffix(".bc"))

	# Compile all C programs
	cDir = (mainDir / "c")
	for cFile in glob.glob("**/*.c", root_dir=cDir, recursive=True):
		cFile = cDir / cFile
		compileC(cFile, calc_output(cFile))

	# Compile all rust programs
	rDir = (mainDir / "rust")
	for rFile in os.listdir(rDir):
		rFile = rDir / rFile
		compileRust(rFile)
	pass

if __name__ == "__main__":
	main()