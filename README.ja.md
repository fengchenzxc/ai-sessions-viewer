<div align="center">

# Claude Session Viewer

[English](README.md) · [中文](README.zh-CN.md) · **日本語** · [CHANGELOG](CHANGELOG.md)

<p align="center"><strong>Claude Code</strong>、<strong>Codex</strong>、<strong>Gemini CLI</strong> 専用のネイティブデスクトップブラウザ。3 つの CLI のローカルセッション履歴を一元的に読み取り、検索し、管理します。</p>

<p align="center">
<strong>忠実な再現</strong> — 思考プロセス、ツール呼び出しのペアリング、構造化 Diff、インライン画像を完全に表示。<br/>
<strong>高効率な検索</strong> — プロジェクトを横断するグローバル検索（<strong>⌘⇧F</strong>）で特定のメッセージへ直行、ワンクリックでターミナルでのセッション再開をサポート。<br/>
<strong>詳細な統計</strong> — ローカル記録に基づいてトークン消費とコストを集計し、アクティビティと費用を多角的に分析（プロジェクト/モデル/ツール呼び出し単位）。<br/>
<strong>読み取り専用の安全性</strong> — オリジナルの JSONL は常に読み取り専用。削除は共有ゴミ箱への移動のみで、物理削除（<code>rm</code>）は一切行いません。<br/>
<strong>柔軟なエクスポート</strong> — 単一または複数セッションをオフラインで読める Markdown / HTML としてエクスポート。
</p>

[![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=fff)](https://tauri.app)
[![Vue 3](https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=fff)](https://vuejs.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<br />

<img src="docs/screenshots/cover.png" alt="Claude Session Viewer — Claude Code・Codex・Gemini CLI セッションの統合ブラウザ" width="820" />

</div>

---

## なぜ作ったか

Claude Code、Codex、Gemini CLI はいずれも会話 JSONL をディスクに保存しますが、レイアウトも CLI も別々で、組み込みのビューワーはありません。本アプリは 3 つを 1 つのタイムラインに統合します。

| Agent | パス | グルーピング |
| --- | --- | --- |
| Claude | `~/.claude/projects/<dir>/<sessionId>.jsonl` | プロジェクトディレクトリ単位 |
| Codex | `~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl` | ファイル内に記録された `cwd` 単位 |
| Gemini | `~/.gemini/tmp/<slug>/chats/session-*.jsonl` | `slug` 単位（cwd は隣接する `.project_root` から読み取り） |

オリジナルファイルに対しては **読み取り専用** です。削除は `~/.claude/.session-viewer-trash/` への移動であり、`rm` は行いません。

## このフォークでの変更点

このリポジトリは [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer) で管理されており、Tauri + Vue + Rust の構成を維持しています。主な追加点は次のとおりです。

- **Codex デスクトップ metadata との照合** — Codex app のスレッド metadata を読み、app 一覧での rank、先頭ページ位置、アーカイブ状態、guardian / auto-review 内部サブスレッドを判定。
- **Codex フィルタとラベル** — 設定から Codex の内部レビューセッションとアーカイブ済みセッションを個別に表示切替でき、一覧上でレビュー / アーカイブとして表示。
- **ターミナル選択** — セッション再開先として Warp、Terminal.app、iTerm2 を選択可能。AppleScript の自動入力ではなく一時実行スクリプトを使うため、アクセシビリティ権限は不要。
- **安全な一時スクリプト** — プロジェクトディレクトリに隠し `.tmpXXXXXX` スクリプトを作り、`codex resume <session_id>` を実行後に自己削除。バックエンド側の遅延削除も併用。
- **Codex 風テーマとフォントサイズ** — Codex ライトテーマ、Dracula ダークテーマ、macOS フォントスムージング、UI 14px / code-diff 12px の基準サイズを追加。
- **統計とパーサー強化** — Codex/Gemini の解析、価格・モデル正規化、ツール/アクティビティ分類、検索キャンセル処理を強化し、回帰テストを追加。
- **リポジトリ移行と自動リリース** — Release / 更新確認リンクを [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer) に変更し、tag push で GitHub Actions がインストーラーを自動ビルド・公開。

## 機能

- 🗂 **統一されたプロジェクトビュー** — CLI を跨いで作業ディレクトリでセッションをまとめる
- 💬 **忠実なチャット再現** — テキスト、thinking ブロック、ツール呼び出し、構造化 diff、画像
- 🔎 **会話内検索 + スコープ絞り込み** — 全体検索のほか、ユーザー発言 / アシスタント応答（編集含む）/ ツール呼び出しのみに絞り込み可能。前後ジャンプとマッチ件数表示
- 🌐 **グローバル検索（⌘⇧F / Ctrl+Shift+F）** — Algolia 風オーバーレイで現在の agent を横断検索。セッションタイトルと自分の発言のみが対象。ヒットをクリックすると該当メッセージへスクロールしてフラッシュ表示。最近の検索は 1 件ずつ削除可能
- 🔃 **セッション一覧の検索と並び替え** — キーワード検索は Rust 側で実行され、タイトル + 自分の発言を横断（入力のたびに前回の検索を即キャンセル）。時刻 / サイズ / メッセージ数で並び替え、ID 付きのみ表示
- 🪗 **ツール呼び出しの一括折りたたみ / 展開** — ノイズを隠して会話本文に集中
- 📤 **セッションのエクスポート** — 単一セッションを Markdown または HTML として保存（ネイティブの "別名で保存"。HTML はアバターとスタイルをインライン化し、オフラインでも開ける）
- 🧰 **複数選択 & 一括操作** — 複数セッションを選んでまとめてゴミ箱へ移動、または `export-YYYYMMDD-HHMMSS-{md,html}/` フォルダーへ一括エクスポート
- 🔄 **再開または新規開始** — プロジェクトで Terminal を開き、既存セッションを再開（`claude --resume <id>` / `codex resume <id>`）するか、新しいセッションを開始
- 📡 **ライブ tail** — 開いているセッションは CLI の追記に合わせて自動更新。"● Live" インジケータで監視中であることを示し、上にスクロール中の追記は "新着 N 件 ↓" バッジに集約
- 🗑 **共有ゴミ箱** — ソフト削除、削除済みセッションの中身をプレビュー、1 件または複数選択で復元。Claude と Codex で共通
- 🏠 **ウェルカム画面** — agent ごとに最近開いたプロジェクトを表示。ワンクリックで再オープン、1 件ずつ削除可能
- 📌 **ピン留め / 沈める** — サイドバー上に色付きドット、沈めたプロジェクトは下に
- ✏️ **セッションのリネーム** — 付け直した名前は CLI にも同期され、`claude` / `codex` の resume ピッカーにも表示される
- 🌗 **ライト / ダーク / システム連動** — Codex 風のニュートラルカラー
- 🌐 **i18n + 自動判定** — 英語 / 简体中文 / 繁體中文 / 日本語。初回起動時に OS の言語に合わせ、該当なしの場合は英語にフォールバック
- ⚡️ **カスタム tooltip & エージェントブランドアイコン** — OS ネイティブの違和感を排除
- 🖼 **画像ライトボックス** — 会話内に貼られたスクリーンショットを拡大表示

## インストール

### ビルド済みバイナリ

[Releases](https://github.com/fengchenzxc/ai-sessions-viewer/releases) から取得：

| プラットフォーム | ファイル |
| --- | --- |
| macOS (Apple Silicon + Intel) | `ai-sessions-viewer_<ver>_universal.dmg` |
| Windows x64 | `ai-sessions-viewer_<ver>_x64-setup.exe` |
| Linux x86_64 (Debian/Ubuntu) | `ai-sessions-viewer_<ver>_amd64.deb` |
| Linux x86_64 (ポータブル) | `ai-sessions-viewer_<ver>_amd64.AppImage` |

macOS 版 `.app` は **ad-hoc 署名済み・未公証** のため、初回起動時に「Apple は…検証できません」というダイアログが出ることがあります。回避方法は 2 つ：

- Finder で `.app` を右クリック → **開く** → ダイアログで再度「開く」を押す（初回のみ）。
- または、ターミナルで隔離属性を外す：
  ```bash
  sudo xattr -dr com.apple.quarantine /Applications/ai-sessions-viewer.app
  ```

Linux 版 `.AppImage` はポータブル形式 —— `chmod +x` で実行可能になります。`.deb` のインストール：
```bash
sudo apt install ./ai-sessions-viewer_<ver>_amd64.deb
```

### ソースからビルド

必要環境：**Node 20+**、**Rust stable**、プラットフォーム別のツールチェーン：
- **macOS** —— Xcode CLT。
- **Windows** —— MSVC + WebView2。
- **Linux** —— `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`librsvg2-dev`、`libxdo-dev`、`patchelf`（Debian/Ubuntu：`sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libxdo-dev patchelf`）。

```bash
git clone https://github.com/fengchenzxc/ai-sessions-viewer.git
cd ai-sessions-viewer
npm install
npm run tauri dev          # 開発モード
npm run tauri build        # .app / .dmg / .msi / .deb / .AppImage をバンドル
```

`npm run build` は型チェック（`vue-tsc --noEmit` + Vite ビルド）です。ユニットテストは `test/` 配下で Vitest を使用 — `npm test` で watch、`npm run test:run` で CI 用の単発実行、`npm run test:coverage` で v8 カバレッジレポート。

## 使い方

1. **エージェント切替** — サイドバー上部のセグメンテッドコントロール（Claude 🟠 / Codex 🟢）
2. **プロジェクトを選ぶ** — サイドバーに全 cwd が並びます。右クリックでピン留め / 沈め / リネーム
3. **セッションを開く** — 中央ペインにメッセージとツール呼び出しが call → result でペアリング表示
4. **再開** — ツールバーの ▶ ボタンが Terminal を開いて該当 CLI を起動
5. **エクスポート** — 詳細ツールバーの ⬇ から単一セッションを Markdown / HTML 保存。一覧で複数選択 → トップバーの ⬇ で `export-YYYYMMDD-HHMMSS-{md,html}/` にまとめて書き出し
6. **削除 / 復元** — ツールバーの 🗑 がソフト削除、トップバーのゴミ箱アイコンから復元

## 一部のスクリーンショット

<table>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/cover.png" alt="メインビュー — サイドバー、セッション、チャット" />
      <p align="center"><em>メインビュー — サイドバー、セッション一覧、チャット、ワンクリックエクスポート</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/chat.png" alt="忠実な再現 — 思考、ツール呼び出し、構造化 Diff" />
      <p align="center"><em>忠実な再現 — 思考、ツール呼び出し、構造化 Diff、ライブ追従</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/search.png" alt="グローバル検索オーバーレイ" />
      <p align="center"><em>グローバル検索（⌘⇧F）で目的のメッセージへ直行</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/stats.png" alt="トークン・コスト分析ダッシュボード" />
      <p align="center"><em>プロジェクト · モデル · ツール別のトークン・コスト分析</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/export.png" alt="ブラウザで開いたエクスポート HTML のプレビュー" />
      <p align="center"><em>エクスポート HTML — 完全オフライン、ブラウザで開ける</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/trash.png" alt="共有ゴミ箱と復元" />
      <p align="center"><em>共有ゴミ箱 — ソフト削除とワンクリック復元</em></p>
    </td>
  </tr>
</table>

## 技術スタック

- **フロントエンド** — Vue 3 + Vite + Tailwind CSS v4（CSS 変数ベースのデザイントークン）
- **バックエンド** — Rust + Tauri 2。各エージェントの JSONL 解析は `SessionSource` トレイト経由で `src-tauri/src/agents/<agent>.rs` に分離
- **JSONL パース** — すべて Rust 側、フロントエンドはディスクに触れない
- **アイコン** — [iconify](https://iconify.design)（`lucide` / `material-icon-theme` / `arcticons`）をビルド時にインライン化
- **ストアなし** — 状態は `App.vue` の ref に置く。`localStorage` は言語 / テーマ / ピン設定のみ

コントリビューター向け資料は [`CLAUDE.md`](CLAUDE.md)（アーキテクチャ）と [`docs/release-ci.md`](docs/release-ci.md)（リリースパイプライン）。

## コントリビュート

PR 歓迎。[Conventional Commits](https://www.conventionalcommits.org/)（`feat:` / `fix:` / `docs:` ...）でお願いします。`release-please` がそれを読んでバージョンを上げ、[`CHANGELOG.md`](CHANGELOG.md) を自動で更新します。

## ライセンス

[MIT](LICENSE) © jerrywu001
