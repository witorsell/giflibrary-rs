#!/usr/bin/env python3
"""Smoke test for caption_gif.py. Run with: python3 scripts/test_caption_gif.py"""
import subprocess
import os
import tempfile
from PIL import Image

SCRIPT = os.path.join(os.path.dirname(__file__), "caption_gif.py")


def make_animated_webp(path, size, n_frames, duration_ms=100):
    frames = [Image.new("RGB", size, (50 * i % 255, 0, 0)) for i in range(n_frames)]
    frames[0].save(
        path,
        format="WEBP",
        save_all=True,
        append_images=frames[1:],
        duration=duration_ms,
        loop=0,
    )


def run_caption(in_path, out_path, caption):
    result = subprocess.run(
        ["python3", SCRIPT, in_path, out_path, caption],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"script failed: {result.stderr}"


def test_static_two_frame_input():
    with tempfile.TemporaryDirectory() as tmp:
        in_path = os.path.join(tmp, "in.webp")
        out_path = os.path.join(tmp, "out.webp")
        make_animated_webp(in_path, (400, 200), n_frames=2)
        run_caption(in_path, out_path, "hello world")

        out = Image.open(out_path)
        assert out.size[0] == 400, f"width changed: {out.size}"
        assert out.size[1] > 200, f"bar not added: {out.size}"
        assert getattr(out, "n_frames", 1) == 2, f"frame count changed: {out.n_frames}"
    print("OK: test_static_two_frame_input")


def test_animated_input_preserves_frame_count():
    with tempfile.TemporaryDirectory() as tmp:
        in_path = os.path.join(tmp, "in.webp")
        out_path = os.path.join(tmp, "out.webp")
        make_animated_webp(in_path, (300, 150), n_frames=6, duration_ms=80)
        run_caption(
            in_path,
            out_path,
            "this is a much longer caption that should wrap across multiple lines for sure",
        )

        out = Image.open(out_path)
        assert getattr(out, "n_frames", 1) == 6, f"frame count changed: {out.n_frames}"
        assert out.size[1] > 150
    print("OK: test_animated_input_preserves_frame_count")


def test_long_caption_grows_bar_taller_than_short_caption():
    with tempfile.TemporaryDirectory() as tmp:
        short_in = os.path.join(tmp, "short_in.webp")
        short_out = os.path.join(tmp, "short_out.webp")
        long_in = os.path.join(tmp, "long_in.webp")
        long_out = os.path.join(tmp, "long_out.webp")
        make_animated_webp(short_in, (400, 200), n_frames=2)
        make_animated_webp(long_in, (400, 200), n_frames=2)

        run_caption(short_in, short_out, "hi")
        run_caption(
            long_in,
            long_out,
            "this caption is deliberately extremely long and repeats itself many many times so that even at the smallest allowed font size it still needs several separate lines to fit within the frame width without running off the edge of the image entirely",
        )

        short_h = Image.open(short_out).size[1]
        long_h = Image.open(long_out).size[1]
        assert long_h > short_h, f"expected long caption bar taller: {long_h} vs {short_h}"
    print("OK: test_long_caption_grows_bar_taller_than_short_caption")


if __name__ == "__main__":
    test_static_two_frame_input()
    test_animated_input_preserves_frame_count()
    test_long_caption_grows_bar_taller_than_short_caption()
    print("All caption_gif.py smoke tests passed")
