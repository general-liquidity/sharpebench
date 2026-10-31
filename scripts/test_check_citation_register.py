#!/usr/bin/env python3
"""Tests for scripts/check-citation-register.py.

Each rule fails on a minimal tree built in a temporary directory, and the last
test runs the checker over the committed register. Credit lines in the fixtures
are assembled at run time so that this file is not itself scanned as a credit.
"""

from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


def load_checker():
    spec = importlib.util.spec_from_file_location(
        "check_citation_register", ROOT / "scripts" / "check-citation-register.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CHECK = load_checker()
AFTER = "After"
CREDIT_TEXT = AFTER + " Example & Author, *A Method* (2020)"

BIB = """@article{example2020method,
  title   = {A Method},
  author  = {Example, A.},
  year    = {2020},
  doi     = {10.1000/example}
}

@misc{plain2021note,
  title = {A Note},
  year  = {2021}
}
"""


class Tree:
    """A paper with two bibliography entries, cited once each, and one source
    file carrying one credit line."""

    def __enter__(self):
        self._dir = tempfile.TemporaryDirectory()
        root = pathlib.Path(self._dir.name)
        (root / "paper" / "sections").mkdir(parents=True)
        (root / "paper" / "refs.bib").write_text(BIB, encoding="utf-8")
        (root / "paper" / "main.tex").write_text("\\input{sections/one}\n", encoding="utf-8")
        (root / "paper" / "sections" / "one.tex").write_text(
            "A method \\citep[p.~4]{example2020method} and a note \\citet{plain2021note}.\n",
            encoding="utf-8",
        )
        source = root / "crates" / "demo" / "src"
        source.mkdir(parents=True)
        (source / "lib.rs").write_text(f"//! {CREDIT_TEXT}.\npub fn f() {{}}\n", encoding="utf-8")
        self.root = root
        return root

    def __exit__(self, *exc):
        self._dir.cleanup()
        return False


def good_rows() -> list[dict]:
    return [
        {
            "kind": "bib",
            "key": "example2020method",
            "identifier": "doi:10.1000/example",
            "mechanism": "The method.",
            "locators": ["paper/sections/one.tex"],
            "pages": ["p.~4"],
        },
        {
            "kind": "bib",
            "key": "plain2021note",
            "identifier": "none",
            "mechanism": "The note.",
            "locators": ["paper/sections/one.tex"],
        },
        {
            "kind": "credit",
            "file": "crates/demo/src/lib.rs",
            "text": CREDIT_TEXT,
            "identifier": "bib:example2020method",
            "mechanism": "The method, in code.",
        },
    ]


def problems_with(mutate) -> list[str]:
    with Tree() as root:
        rows = good_rows()
        mutate(rows, root)
        return CHECK.validate(rows, root)


class Register(unittest.TestCase):
    def assertFlags(self, problems, needle):
        self.assertTrue(any(needle in p for p in problems), problems)

    def test_the_fixture_register_passes(self):
        self.assertEqual(problems_with(lambda rows, root: None), [])

    def test_a_bib_key_missing_from_the_register_fails(self):
        problems = problems_with(lambda rows, root: rows.pop(1))
        self.assertFlags(problems, "refs.bib key plain2021note has no register entry")

    def test_a_register_entry_for_no_bib_key_fails(self):
        problems = problems_with(lambda rows, root: rows[1].update(key="gone2019"))
        self.assertFlags(problems, "names no key in paper/refs.bib")

    def test_an_identifier_the_bib_does_not_carry_fails(self):
        problems = problems_with(lambda rows, root: rows[0].update(identifier="arXiv:1234.5678"))
        self.assertFlags(problems, "is not the one refs.bib carries")

    def test_a_missing_mechanism_fails(self):
        problems = problems_with(lambda rows, root: rows[0].update(mechanism=" "))
        self.assertFlags(problems, "no credited mechanism")

    def test_a_locator_that_does_not_cite_the_key_fails(self):
        def mutate(rows, root):
            (root / "paper" / "main.tex").write_text("no citations here\n", encoding="utf-8")
            rows[0]["locators"].append("paper/main.tex")

        self.assertFlags(problems_with(mutate), "paper/main.tex does not cite example2020method")

    def test_a_citing_file_left_out_of_the_locators_fails(self):
        def mutate(rows, root):
            (root / "paper" / "sections" / "two.tex").write_text(
                "\\cite{plain2021note}\n", encoding="utf-8"
            )

        self.assertFlags(problems_with(mutate), "two.tex cites plain2021note but is not among")

    def test_an_unrecorded_or_invented_page_fails(self):
        self.assertFlags(
            problems_with(lambda rows, root: rows[0].pop("pages")), "which is not recorded"
        )
        self.assertFlags(
            problems_with(lambda rows, root: rows[0].update(pages=["p.~9"])),
            "gives the page locator 'p.~9'",
        )

    def test_an_uncited_entry_needs_a_code_credit_that_names_it(self):
        def code_only(rows, root):
            (root / "paper" / "sections" / "one.tex").write_text(
                "A method \\citep[p.~4]{example2020method}.\n", encoding="utf-8"
            )
            rows[1]["locators"] = []

        problems = problems_with(code_only)
        self.assertFlags(problems, "no code credit names plain2021note")

        def credited(rows, root):
            code_only(rows, root)
            rows[2]["identifier"] = "bib:plain2021note"

        self.assertEqual(problems_with(credited), [])

    def test_an_empty_locator_list_does_not_hide_a_citing_file(self):
        problems = problems_with(lambda rows, root: rows[0].update(locators=[]))
        self.assertFlags(problems, "one.tex cites example2020method but is not among")

    def test_a_credit_line_with_no_entry_fails(self):
        problems = problems_with(lambda rows, root: rows.pop(2))
        self.assertFlags(problems, "credit line has no register entry")

    def test_a_credit_entry_that_points_at_nothing_fails(self):
        problems = problems_with(lambda rows, root: rows[2].update(text=AFTER + " Nobody"))
        self.assertFlags(problems, "no credit line in crates/demo/src/lib.rs carries")
        problems = problems_with(lambda rows, root: rows[2].update(file="crates/demo/src/gone.rs"))
        self.assertFlags(problems, "is not on disk")

    def test_a_credit_identifier_must_resolve_or_say_why_not(self):
        problems = problems_with(lambda rows, root: rows[2].update(identifier="bib:gone2019"))
        self.assertFlags(problems, "names no key in paper/refs.bib")
        problems = problems_with(lambda rows, root: rows[2].update(identifier="unresolved"))
        self.assertFlags(problems, "needs a note")
        ok = problems_with(
            lambda rows, root: rows[2].update(identifier="unresolved", note="Not in refs.bib.")
        )
        self.assertEqual(ok, [])


class CommittedRegister(unittest.TestCase):
    def test_the_committed_register_passes(self):
        self.assertEqual(CHECK.main(), 0)


if __name__ == "__main__":
    unittest.main()
