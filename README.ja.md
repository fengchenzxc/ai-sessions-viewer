<div align="center">

# Claude Session Viewer

[English](README.md) · [中文](README.zh-CN.md) · **日本語** · [CHANGELOG](CHANGELOG.md)

**Claude Code** と **Codex** のローカルセッションログを閲覧するためのデスクトップアプリ。2 つの CLI の会話履歴を 1 つのタイムラインで読み・検索・再開・ソフト削除できます。

[![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=fff)](https://tauri.app)
[![Vue 3](https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=fff)](https://vuejs.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

---

## なぜ作ったか

Claude Code と Codex は会話 JSONL をディスクに保存しますが、レイアウトも CLI も別々で、組み込みのビューワーはありません。本アプリは両者を 1 つのタイムラインに統合します。

| Agent | パス | グルーピング |
| --- | --- | --- |
| Claude | `~/.claude/projects/<dir>/<sessionId>.jsonl` | プロジェクトディレクトリ単位 |
| Codex | `~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl` | ファイル内に記録された `cwd` 単位 |

オリジナルファイルに対しては **読み取り専用** です。削除は `~/.claude/.session-viewer-trash/` への移動であり、`rm` は行いません。

## 機能

- 🗂 **統一されたプロジェクトビュー** — CLI を跨いで作業ディレクトリでセッションをまとめる
- 💬 **忠実なチャット再現** — テキスト、thinking ブロック、ツール呼び出し、構造化 diff、画像
- 🔎 **会話内検索 + スコープ絞り込み** — 全体検索のほか、ユーザー発言 / アシスタント応答（編集含む）/ ツール呼び出しのみに絞り込み可能。前後ジャンプとマッチ件数表示
- 🔃 **セッション一覧の検索と並び替え** — キーワードでプロジェクト内のセッションを絞り込み（一致箇所をハイライト）、時刻 / サイズ / メッセージ数で並び替え、ID 付きのみ表示
- 🪗 **ツール呼び出しの一括折りたたみ / 展開** — ノイズを隠して会話本文に集中
- 📤 **セッションのエクスポート** — 単一セッションを Markdown または HTML として保存（ネイティブの "別名で保存"。HTML はアバターとスタイルをインライン化し、オフラインでも開ける）
- 🔄 **再開または新規開始** — プロジェクトで Terminal を開き、既存セッションを再開（`claude --resume <id>` / `codex resume <id>`）するか、新しいセッションを開始
- 🗑 **共有ゴミ箱** — ソフト削除、削除済みセッションの中身をプレビュー、1 件または複数選択で復元。Claude と Codex で共通
- 📌 **ピン留め / 沈める** — サイドバー上に色付きドット、沈めたプロジェクトは下に
- ✏️ **セッションのリネーム** — 付け直した名前は CLI にも同期され、`claude` / `codex` の resume ピッカーにも表示される
- 🌗 **ライト / ダーク / システム連動** — Codex 風のニュートラルカラー
- 🌐 **i18n + 自動判定** — 英語 / 简体中文 / 繁體中文 / 日本語。初回起動時に OS の言語に合わせ、該当なしの場合は英語にフォールバック
- ⚡️ **カスタム tooltip & エージェントブランドアイコン** — OS ネイティブの違和感を排除
- 🖼 **画像ライトボックス** — 会話内に貼られたスクリーンショットを拡大表示

## スクリーンショット

> _(後日 `docs/screenshots/` に追加)_

## インストール

### ビルド済みバイナリ

[Releases](https://github.com/wuchao/claude-session-viewer/releases) から取得：

| プラットフォーム | ファイル |
| --- | --- |
| macOS (Apple Silicon + Intel) | `claude-session-viewer_<ver>_universal.dmg` |
| Windows x64 | `claude-session-viewer_<ver>_x64-setup.exe` |

macOS の未署名 `.app` を初回起動するときは、右クリック → **開く** で Gatekeeper を回避してください。

### ソースからビルド

必要環境：**Node 20+**、**Rust stable**、**Xcode CLT**（macOS）または **MSVC + WebView2**（Windows）。

```bash
git clone https://github.com/wuchao/claude-session-viewer.git
cd claude-session-viewer
npm install
npm run tauri dev          # 開発モード
npm run tauri build        # .app / .dmg / .msi をバンドル
```

`npm run build` は型チェック（`vue-tsc --noEmit` + Vite ビルド）です。テストランナーは含まれていません。

## 使い方

1. **エージェント切替** — サイドバー上部のセグメンテッドコントロール（Claude 🟠 / Codex 🟢）
2. **プロジェクトを選ぶ** — サイドバーに全 cwd が並びます。右クリックでピン留め / 沈め / リネーム
3. **セッションを開く** — 中央ペインにメッセージとツール呼び出しが call → result でペアリング表示
4. **再開** — ツールバーの ▶ ボタンが Terminal を開いて該当 CLI を起動
5. **削除 / 復元** — ツールバーの 🗑 がソフト削除、トップバーのゴミ箱アイコンから復元

## 技術スタック

- **フロントエンド** — Vue 3 + Vite + Tailwind CSS v4（CSS 変数ベースのデザイントークン）
- **バックエンド** — Rust + Tauri 2。各エージェントの JSONL 解析は `SessionSource` トレイト経由で `src-tauri/src/agents/<agent>.rs` に分離
- **JSONL パース** — すべて Rust 側、フロントエンドはディスクに触れない
- **アイコン** — [iconify](https://iconify.design)（`lucide` / `material-icon-theme` / `arcticons`）をビルド時にインライン化
- **ストアなし** — 状態は `App.vue` の ref に置く。`localStorage` は言語 / テーマ / ピン設定のみ

コントリビューター向け資料は [`CLAUDE.md`](CLAUDE.md)（アーキテクチャ）と [`docs/release-ci.md`](docs/release-ci.md)（リリースパイプライン）。

## ロードマップ

- [ ] Gemini CLI セッション対応（次の予定）
- [ ] トークン使用量・コスト分析 —— メッセージ / セッション / プロジェクト単位
- [ ] 統計概要ダッシュボード —— アクティビティ、モデル・トークン内訳
- [ ] セッション横断の全文検索
- [ ] セッションのお気に入り・タグ
- [ ] ライブ tail —— 進行中のセッションを自動更新
- [ ] 一括エクスポート / 削除 —— _複数選択での復元は完了；一括エクスポート / 削除は未対応_
- [ ] キーボードショートカットとネイティブメニュー —— _⌘F / Ctrl+F 検索は完了；ネイティブメニューは未対応_
- [ ] Linux ビルドターゲット（+ Homebrew / AppImage）
- [ ] Tauri 自動更新 —— _手動「アップデートを確認」は完了；サイレント自動更新は未対応_

## コントリビュート

PR 歓迎。[Conventional Commits](https://www.conventionalcommits.org/)（`feat:` / `fix:` / `docs:` ...）でお願いします。`release-please` がそれを読んでバージョンを上げ、[`CHANGELOG.md`](CHANGELOG.md) を自動で更新します。

## ライセンス

[MIT](LICENSE) © wuchao
