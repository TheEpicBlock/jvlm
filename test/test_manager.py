#!/bin/env python

from abc import ABC, abstractmethod
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
import glob
import re
from typing import Callable, Literal, Protocol, TypeVar, overload, override
import subprocess
import os
import shutil
import sys
import typing
from zipfile import ZipFile

mainDir = Path(__file__).parent.resolve()

JSHELL = "jshell"
CLANG = "clang"
CARGO = "cargo"
CODEGEN_BACKEND = mainDir / "../target/debug/librustc_codegen_jvlm.so"

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

class Failure(ABC):
	@abstractmethod
	def short_err(self) -> str:
		...
	def full_err(self) -> str:
		return self.short_err()
@dataclass
class TimeoutFailure(Failure):
	cmd: str
	time: float
	@override
	def short_err(self) -> str:
		return f"Command {self.cmd} timed out after {self.time} seconds"
@dataclass
class NoTimeoutFailure(Failure):
	time: float
	@override
	def short_err(self) -> str:
		return f"Test did not time out after {self.time} seconds"
@dataclass
class AssertFailure(Failure):
	expected: str
	found: str
	@override
	def short_err(self) -> str:
		return f"Expected '{self.expected}', found '{self.found}'"
@dataclass
class StatusCodeFailure(Failure):
	cmd: str
	code: int
	stdout: bytes
	stderr: bytes
	@override
	def short_err(self) -> str:
		return f"Command {self.cmd} failed with status code {self.code}"
	@override
	def full_err(self) -> str:
		return f"Command {self.cmd} failed with status code {self.code}.\nStderr:\n{self.stderr.decode(errors="ignore").replace("\n","\n  ")}\nStdout:\n{self.stdout.decode(errors="ignore").replace("\n","\n  ")}."
@dataclass
class CompileFailure(Failure):
	lang_name: str
	segment: str
	inner: Failure
	@override
	def short_err(self) -> str:
		return f"Failed to compile {self.lang_name}/{self.segment}: {self.inner.short_err()}"
	@override
	def full_err(self) -> str:
		return f"Failed to compile {self.lang_name}/{self.segment}: {self.inner.full_err()}"
@dataclass
class TestFailure(Failure):
	lang_name: str
	segment: str
	inner: Failure
	@override
	def short_err(self) -> str:
		return f"Test failure {self.lang_name}/{self.segment}: {self.inner.short_err()}"
	@override
	def full_err(self) -> str:
		return f"Test failure {self.lang_name}/{self.segment}: {self.inner.full_err()}"

@overload
def exec(args: list[str | Path], return_output: Literal[True], cwd: str | bytes | os.PathLike[str] | os.PathLike[bytes] | None = None, input: str | None = None, timeout: float | None = None) -> Failure | str:
	...
@overload
def exec(args: list[str | Path], return_output: Literal[False] = False, cwd: str | bytes | os.PathLike[str] | os.PathLike[bytes] | None = None, input: str | None = None, timeout: float | None = None) -> Failure | None:
	...
def exec(args: list[str | Path], return_output: bool = False, cwd: str | bytes | os.PathLike[str] | os.PathLike[bytes] | None = None, input: str | None = None, timeout: float | None = None) -> Failure | str | None:
	try:
		r = subprocess.run(args, cwd=cwd, timeout=timeout, capture_output=True, input=(input.encode() if input is not None else None))
		if r.returncode != 0:
			return StatusCodeFailure(str(args[0]), r.returncode, r.stdout, r.stderr)
		if return_output:
			return r.stdout.decode(errors="replace") + r.stderr.decode(errors="replace")
		else:
			return None
	except subprocess.TimeoutExpired:
		return TimeoutFailure(str(args[0]), -1 if timeout is None else timeout)

class Language(ABC):
	name: str
	base_dir: Path

	def initialize(self, name: str, mainDir: Path):
		self.name = name
		self.base_dir = mainDir / self.name

	@abstractmethod
	def normalize_test_segment(self, test: str) -> str | None:
		"""
		Normalizes the segment of a test. If this in not a valid test, this function returns none.
		
		Examples:
		RustLanguage().normalize_test("example/cargo.toml") == "example"
		CLanguage().normalize_test("example.c") == "example.c"
		CLanguage().normalize_test("example.txt") == "None"
		"""
		...
	@abstractmethod
	def list_all_tests(self, dir: str = "", recurse: bool = True) -> Iterable[str]:
		"""
		Creates a list of all segments for all tests for this lang. Optional parameters to only list
		tests in a certain directory, and whether to recurse into subdirectories.

		Examples:
		list(CLanguage().list_all_tests()) == ["example.c", "example2.c"]
		"""
		...
	@abstractmethod
	def compileJar(self, segment: str) -> Path | Failure:
		...
	@abstractmethod
	def showLlvmir(self, segment: str):
		...
	@abstractmethod
	def get_tests_declaration(self, segment: str) -> str:
		...

class RustLanguage(Language):
	@override
	def normalize_test_segment(self, test: str) -> str | None:
		# We don't support nested directories yet
		normalized_test = test.split("/")[0]
		if (self.base_dir / normalized_test / "Cargo.toml").exists():
			return normalized_test
		else:
			return None
	@override
	def list_all_tests(self, dir: str = "", recurse: bool = True) -> Iterable[str]:
		if dir != "":
			# No support for nested directories yet, which means that any non-root path cannot have tests
			return []
		return (f"{f}" for f in os.listdir(self.base_dir) if (self.base_dir / f / "Cargo.toml").exists())
	@override
	def compileJar(self, segment: str) -> Path | Failure:
		...
	@override
	def showLlvmir(self, segment: str):
		...
	@override
	def get_tests_declaration(self, segment: str) -> str:
		return ""

class CLanguage(Language):
	@override
	def normalize_test_segment(self, test: str) -> str | None:
		# Common extensions for non-tests which might accidentally end up in the test directory
		if test.endswith(".bc") or test.endswith(".ll") or test.endswith(".o") or test.endswith(".so") or test.endswith(".out") :
			return None
		if not test.endswith(".c"):
			# Tests for this language should end with .c, try appending one
			test = f"{test}.c"
		return test if (self.base_dir / test).exists() else None

	@override
	def list_all_tests(self, dir: str = "", recurse: bool = True) -> Iterable[str]:
		return (f"{f}" for f in glob.glob("**/*.c" if recurse else "*.c", root_dir=(self.base_dir / dir), recursive=recurse))

	@override
	def get_tests_declaration(self, segment: str) -> str:
		input_file = self.base_dir / segment
		return next(re.finditer("/\\*(.*)\\*/", input_file.read_text(), re.MULTILINE | re.DOTALL)).group(1)

	def get_compile_flags(self, segment: str) -> list[str]:
		# tests declaration also includes compiler flags
		cflags = next(re.finditer("^compile\\s(.*)$", self.get_tests_declaration(segment), re.MULTILINE)).group(1)
		cflags = ("" if not isinstance(cflags, str) else cflags)
		cflags = cflags.split(" ")
		return cflags

	@override
	def compileJar(self, segment: str) -> Path | Failure:
		input_file = self.base_dir / segment
		tmp_file = mainDir / "out" / "c" / (segment.removesuffix(".c")+".bc")
		tmp_file.parent.mkdir(parents=True, exist_ok=True)

		r = exec([
			CLANG,
		] + self.get_compile_flags(segment) + [
			"-emit-llvm",
			"-c",
			input_file,
			"-o",
			tmp_file
		])

		if r != None:
			return r

		jar_file = mainDir / "out" / "c" / (segment.removesuffix(".c")+".jar")
		r = exec([
			CARGO,
			"run",
			"--quiet",
			"--",
			tmp_file,
			jar_file
		], cwd=(mainDir / ".."))

		if r != None:
			return r
		return jar_file

	@override
	def showLlvmir(self, segment: str):
		input_file = self.base_dir / segment
		_ = subprocess.check_call([
			CLANG,
		] + self.get_compile_flags(segment) + [
			"-S",
			"-emit-llvm",
			"-c",
			input_file,
			"-o",
			"-"
		])

languages = {
	"c": CLanguage(),
	"rust": RustLanguage()
}
for (k,v) in languages.items():
	v.initialize(k, mainDir)


def parse_test_declaration(declaration: str) -> list[Callable[[Path], Failure | None]]:
	tests: list[Callable[[Path], Failure | None]] = []
	lines = declaration.splitlines()
	i = 0
	while i < len(lines):
		cLine = lines[i]
		if cLine.startswith("java_run"):
			run_str = cLine.removeprefix("java_run").strip()
			nLine = lines[i+1]
			if nLine.startswith("expect_timeout"):
				expect = nLine.removeprefix("expect_timeout").removeprefix("expect_timeout").strip()
				if not expect.endswith("seconds"):
					raise Exception("Invalid syntax for expect_timeout")
				timeout = float(expect.removesuffix("seconds").strip())
				def run_test(p: Path) -> Failure | None:
					try:
						_ = subprocess.run([JSHELL, "--enable-preview", "-c", p, "-"], capture_output=True, input=f"System.out.println({run_str})".encode(), timeout=timeout)
						return NoTimeoutFailure(timeout)
					except subprocess.TimeoutExpired:
						return None
				tests.append(run_test)
			elif nLine.startswith("expect"):
				contains = nLine.startswith("expect_contains")
				if contains:
					expect = nLine.removeprefix("expect_contains").strip()
				else:
					expect = nLine.removeprefix("expect").strip()
				def run_test(p: Path) -> Failure | None:
					r = exec([JSHELL, "-c", p, "-"], input=f"System.out.println({run_str})", return_output=True, timeout=300)
					if isinstance(r, Failure):
						return r
					else:
						if (not contains and r.strip() != expect) or (contains and (not expect in r.strip())):
							return AssertFailure(expect, r.strip())
						else:
							return None
				tests.append(run_test)
			else:
				raise Exception("java_run has no expect")
		i += 1
	return tests

def main():
	if len(sys.argv) <= 1:
		print(f"Usage: {sys.argv[0]} <mode> [tests...]")
		sys.exit(1)
	mode = sys.argv[1]
	match mode:
		case "dry_run":
			pass
		case "show_ir":
			pass
		case "javap":
			pass
		case "jar":
			pass
		case "test":
			pass
		case _:
			print(f"Unknown mode {mode}")
			sys.exit(1)

	# Plan out which tests we need to operate on
	tests: Iterable[tuple[Language, str]]
	if len(sys.argv[2:]) == 0:
		tests = ((l, test) for (l) in languages.values() for test in l.list_all_tests())
	else:
		def a() -> Iterable[tuple[Language, str]]:
			weird_dir = Path(".").absolute() != mainDir # If true, the arguments might be relative to pwd
			for arg in sys.argv[2:]:
				if arg.startswith("./"):
					arg = arg[2:]
				lang = None
				for candidate in languages.keys():
					if arg.startswith(candidate) or arg.startswith("/"+candidate):
						lang = candidate
						break
				if lang is None and weird_dir and Path(arg).exists():
					arg = str(Path(arg).absolute().relative_to(mainDir, walk_up=True))
					for candidate in languages.keys():
						if arg.startswith(candidate) or arg.startswith("/"+candidate):
							lang = candidate
							break
				if lang is not None:
					# The part of the name after the lang
					segment = arg[len(lang):]
					l = languages[lang]
					if segment == "" or segment == "/":
						# That means to just run all the tests of the lang
						yield from ((l, s) for s in l.list_all_tests())
					else:
						if segment.startswith("/"):
							segment = segment.removeprefix("/")
							# Support basic glob syntax
							if segment.endswith("**/*"):
								segment = segment.removesuffix("**/*")
								yield from ((l, s) for s in l.list_all_tests(dir=segment, recurse=True))
							elif segment.endswith("*"):
								segment = segment.removesuffix("*")
								yield from ((l, s) for s in l.list_all_tests(dir=segment, recurse=False))
							else:
								segment = l.normalize_test_segment(segment)
								if segment is not None:
									yield (l, segment)
						else:
							# No /, that means the arg was something like "rustabcdefg", which is invalid
							# Just ignore that
							pass
		tests = unique(a())

	def do_jar(tests: Iterable[tuple[Language, str]]) -> Iterable[tuple[tuple[Language, str], Path | Failure]]:
		for (l, s) in tests:
			v = l.compileJar(s)
			if isinstance(v, Failure):
				yield ((l,s),CompileFailure(l.name, s, v))
			else:
				yield ((l,s),v)
	
	# Alrighty, we know which tests to operate on now
	match mode:
		case "dry_run":
			print(list(f"{l.name}/{s}" for (l, s) in tests))
			pass
		case "show_ir":
			for (l, s) in tests:
				l.showLlvmir(s)
		case "javap":
			import zipfile
			jars: list[Path] = []
			failures: list[Failure] = []
			for (_, j) in do_jar(tests):
				if isinstance(j, Failure):
					failures.append(j)
				else:
					jars.append(j)
			if len(failures) > 0:
				for f in failures:
					print(f"!! {f.short_err()}")
				sys.exit(2)
			classes: list[str] = []
			for jar in jars:
				z = ZipFile(jar)
				for cl in z.filelist:
					classes.append(f"jar:{jar.as_uri()}!/{cl.filename}")
			sys.exit(subprocess.check_call(["javap", "-c"] + classes))
		case "jar":
			failures: list[Failure] = []
			for (_, j) in do_jar(tests):
				if isinstance(j, Failure):
					failures.append(j)
				else:
					print(j)
			if len(failures) == 1:
				print(f"!! {failures[0].full_err()}")
			elif len(failures) > 1:
				for f in failures:
					print(f"!! {f.short_err()}")
			sys.exit(2 if len(failures) > 0 else 0)
		case "test":
			failures: list[Failure] = []
			for (tst, j) in do_jar(tests):
				if isinstance(j, Failure):
					failures.append(j)
				else:
					# test declaration contains which tests to do
					test_declaration = tst[0].get_tests_declaration(tst[1])
					test_runners = parse_test_declaration(test_declaration)
					anyfail = False
					for r in test_runners:
						result = r(j)
						if result is not None:
							failures.append(TestFailure(tst[0].name, tst[1], result))
							anyfail = True
					if not anyfail:
						print(f"{tst[0].name}/{tst[1]}: Success on {len(test_runners)} tests")
			if len(failures) == 1:
				print(f"!! {failures[0].full_err()}")
			elif len(failures) > 1:
				for f in failures:
					print(f"!! {f.short_err()}")

T = TypeVar('T')
def unique(iter: Iterable[T]) -> Iterable[T]:
	s: set[T] = set()
	for n in iter:
		if not (n in s):
			yield n
			s.add(n)

if __name__ == "__main__":
	main()