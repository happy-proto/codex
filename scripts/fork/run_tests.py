#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///

"""在 uv 准备的依赖环境中运行 fork 的脚本测试。"""

import sys
import unittest
from pathlib import Path
from typing import Annotated

import typer

app = typer.Typer(add_completion=False, pretty_exceptions_enable=False)


@app.command()
def main(
    tests: Annotated[
        list[str] | None,
        typer.Argument(help="模块、测试类或方法名；省略则发现所有匹配测试"),
    ] = None,
    pattern: Annotated[
        str, typer.Option("--pattern", help="测试文件 glob；仅在未指定 tests 时使用")
    ] = "test_*.py",
) -> None:
    """运行脚本测试，成功退出 0，失败退出 1。"""
    directory = Path(__file__).resolve().parent
    sys.path.insert(0, str(directory))
    loader = unittest.TestLoader()
    suite = (
        loader.loadTestsFromNames(tests)
        if tests
        else loader.discover(str(directory), pattern=pattern)
    )
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise typer.Exit(0 if result.wasSuccessful() else 1)


if __name__ == "__main__":
    app()
