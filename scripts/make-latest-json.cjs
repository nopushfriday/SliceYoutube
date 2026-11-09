// 자동 업데이트용 latest.json 생성. 앱(tauri-plugin-updater)이 이 파일로 새 버전을 확인한다.
// 사용: VER=1.2.3 REPO=owner/name node scripts/make-latest-json.cjs <release-files 폴더>
const fs = require('fs');
const path = require('path');

const dir = process.argv[2];
const { VER, REPO } = process.env;
const files = fs.readdirSync(dir, { recursive: true }).map((f) => path.join(dir, f)).filter((f) => fs.statSync(f).isFile());

const entry = (suffix) => {
  const file = files.find((f) => f.endsWith(suffix));
  const sig = files.find((f) => f.endsWith(`${suffix}.sig`));
  if (!file || !sig) throw new Error(`업데이트 파일 또는 서명이 없습니다: *${suffix}`);
  return {
    signature: fs.readFileSync(sig, 'utf8').trim(),
    url: `https://github.com/${REPO}/releases/download/v${VER}/${path.basename(file)}`
  };
};

const latest = {
  version: VER,
  notes: '',
  pub_date: new Date().toISOString(),
  platforms: { 'windows-x86_64': entry('-setup.exe'), 'linux-x86_64': entry('.AppImage'), 'darwin-aarch64': entry('.app.tar.gz') }
};
fs.writeFileSync(path.join(dir, 'latest.json'), JSON.stringify(latest, null, 2));
