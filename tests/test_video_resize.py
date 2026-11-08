"""Videos in Resize slides: the video is never modified and keeps its proportion;
behind it, a blurred, silent copy covers the whole slide and plays with it."""
import hashlib, json, os, re, shutil, subprocess, tempfile, unittest, zipfile
from pathlib import Path
from test_guards import BIN

FFMPEG = shutil.which('ffmpeg') or next((p for p in ['/opt/homebrew/bin/ffmpeg', '/usr/local/bin/ffmpeg'] if os.path.exists(p)), None)
WINDOWS_ENGINE = 'windows-target' in str(BIN)
NAME = 'PPT Show Tools blurred background'

@unittest.skipUnless(FFMPEG, 'FFmpeg is needed to make the test video')
class VideoResize(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = Path(tempfile.mkdtemp())
        subprocess.run([FFMPEG, '-nostdin', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=25', '-f', 'lavfi', '-i', 'sine=frequency=440', '-t', '2', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest', str(cls.dir / 'clip.mp4')], check=True)
        subprocess.run([FFMPEG, '-nostdin', '-loglevel', 'error', '-i', str(cls.dir / 'clip.mp4'), '-frames:v', '1', str(cls.dir / 'poster.png')], check=True)
        subprocess.run(['python3', str(Path(__file__).with_name('video_deck.py')), str(cls.dir), str(cls.dir / 'video.pptx')], check=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.dir, ignore_errors=True)

    def resize(self, name, **env):
        out = self.dir / name
        r = subprocess.run([str(BIN), '--resize', '--size', '3427x534', str(out), str(self.dir / 'video.pptx')], capture_output=True, text=True, env={**os.environ, **env})
        self.assertEqual(r.returncode, 0, r.stderr)
        with zipfile.ZipFile(out) as z:
            parts = {n: z.read(n) for n in z.namelist()}
        return parts, json.loads(r.stdout)

    def test_video_is_kept_intact_over_a_moving_blurred_copy(self):
        parts, report = self.resize('moving.pptx')
        self.assertEqual(hashlib.sha256(parts['ppt/media/media1.mp4']).digest(), hashlib.sha256((self.dir / 'clip.mp4').read_bytes()).digest())
        slide = parts['ppt/slides/slide1.xml'].decode()
        frames = re.findall(r'name="([^"]+)".*?<a:off x="(\d+)" y="(\d+)"/><a:ext cx="(\d+)" cy="(\d+)"/>', slide)
        self.assertEqual([f[0] for f in frames], [NAME, 'Clip'])  # the copy is behind
        self.assertEqual(frames[0][1:], ('0', '0', '32642175', '5086350'))
        self.assertEqual(frames[1][1:], ('11799888', '0', '9042400', '5086350'))  # 16:9, centred
        copies = [n for n in parts if re.fullmatch(r'ppt/media/pst-blur-[0-9a-f]{8}\.mp4', n)]
        self.assertEqual(len(copies), 1)
        probe = subprocess.run([FFMPEG.replace('ffmpeg', 'ffprobe'), '-v', 'error', '-show_entries', 'stream=codec_type,width,height', '-of', 'csv=p=0', '-'], input=parts[copies[0]], capture_output=True).stdout.decode().split()
        self.assertEqual(probe, ['video,2310,360'])  # silent, slide proportion
        self.assertEqual(slide.count('<p:cmd '), 2)
        self.assertIn('<p:cMediaNode vol="0" mute="1">', slide)
        self.assertIn('Extension="mp4"', parts['[Content_Types].xml'].decode())
        self.assertFalse(any('blurred still' in l for l in report['limitations']), report['limitations'])

    @unittest.skipUnless(WINDOWS_ENGINE, 'macOS always makes the moving copy itself')
    def test_without_ffmpeg_a_blurred_still_is_used(self):
        parts, report = self.resize('still.pptx', PPTSHOWTOOLS_FFMPEG=str(self.dir / 'missing'))
        slide = parts['ppt/slides/slide1.xml'].decode()
        backdrop = re.search(r'<p:pic><p:nvPicPr><p:cNvPr id="\d+" name="' + NAME + r'"/>.*?</p:pic>', slide).group(0)
        self.assertNotIn('videoFile', backdrop)
        self.assertEqual(slide.count('<p:cmd '), 1)
        self.assertFalse([n for n in parts if n.endswith('.mp4') and 'pst-blur' in n])
        self.assertTrue(any('Install FFmpeg' in l for l in report['limitations']), report['limitations'])

    def test_merge_with_a_video_deck_passes_its_checks(self):
        out = self.dir / 'merged.pptx'
        r = subprocess.run([str(BIN), '--merge', '--size', '3427x534', str(out), str(self.dir / 'video.pptx'), str(self.dir / 'video.pptx')], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        report = json.loads(Path(str(out) + '.report.json').read_text())
        self.assertTrue(all(not v['issues'] for v in report['validation']), report['validation'])

if __name__ == '__main__':
    unittest.main()
