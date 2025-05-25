import inspect
import itertools

import sys
import traceback


def line_number_from_traceback() -> int:
    tb = sys.exc_info()[2]
    frames = traceback.extract_tb(tb)

    for frame in frames:
        if frame.filename == "<exec>":
            return frame.lineno

    return frames[-1].lineno


def get_user_frames() -> list[inspect.FrameInfo] | None:
    rev_frames = reversed(inspect.stack())
    try:
        rev_frames = itertools.dropwhile(
            lambda frame: not ("/_pyodide/" in frame.filename and frame.function == "eval_code_async"),
            rev_frames,
        )
        rev_frames = itertools.dropwhile(
            lambda frame: frame.filename != "<exec>", rev_frames
        )
        rev_frames = itertools.takewhile(
            lambda frame: frame.filename == "<exec>", rev_frames
        )

        return list(reversed(list(rev_frames)))
    except:
        return None


def get_return_line(code: str) -> int:
    code = code.rstrip()
    return code.count("\n") + 1
