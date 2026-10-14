# WPF AvalonDock MVVM + DI Sample

Dirkster.AvalonDock を MVVM (CommunityToolkit.Mvvm) と DI (Microsoft.Extensions.DependencyInjection) で使うサンプル。

- 上部の水平トグルボタンで、下のドッキングスペースにペインを表示/非表示
  - 「メモ / ドキュメント2」: 中央のドキュメント領域にタブで表示
  - 「エクスプローラー / プロパティ / 出力」: 左 / 右 / 下に表示
  - ドキュメントもツールも同じ仕組み（AvalonDock の Anchorable + IsVisible）で扱う
  - ボタンの表示名（`ButtonText`）とタブ/フローティングのタイトル（`Title`）は別々に指定できる
  - ペインの追加は `App.xaml.cs` に `AddSingleton<PaneViewModel>` を1行 + `App.xaml` に DataTemplate
- ペインはドラッグでドッキング位置の変更・フローティングが可能
- 「設定」ペイン：中に左タブ（TabStripPlacement=Left）を持つペインの例
  - タブ名は左寄せ、`\n` で改行、幅を超えたら自動折り返し
  - 入力エラーがあるタブに赤い「!」を表示（ObservableValidator + DataAnnotations）。ツールチップにエラー内容
  - 「全般」の「自動保存する」をチェックすると、下に保存間隔などの詳細設定が出る（OFF の間は隠れた項目のエラーを数えない）
  - どこかのタブにエラーがあると、上部の「設定」ボタンにも同じ印を表示
- 共通スタイル（`Styles/`）
  - `Typography.xaml`: フォント・文字サイズ・色と、文字スタイル（Text.Title / Heading / Label / Body / Caption / Error / Code）
  - `Controls.xaml`: 入力欄・一覧のスタイル（Input.TextBox / Input.Code / Input.CheckBox / List.Plain / List.Code）
  - 使い方: `<TextBlock Style="{StaticResource Text.Heading}" />`
  - 文字サイズは DynamicResource なので、設定ペインの「文字サイズ」を変えるとアプリ全体に即反映
- 数値入力の部品（`Controls/NumericUpDown`）
  - テキストボックス＋上下ボタン。↑↓キーでも増減、押しっぱなしで連続増減
  - PageUp/PageDown とマウスホイール（フォーカス時）は `LargeIncrement`（未指定なら Increment の10倍）ずつ。Ctrl＋ホイールは Increment ずつの微調整
  - `Minimum` / `Maximum` の外には出ない（端に達したボタンは無効）。`Increment` で増減幅、`DecimalPlaces` で小数の桁数
  - 使い方: `<controls:NumericUpDown Value="{Binding X}" Minimum="0" Maximum="10" Increment="0.1" DecimalPlaces="1" Style="{StaticResource Input.Numeric}" />`
- 設定の保存（`Settings/`, `Services/SettingsService.cs`）
  - 設定ペイン下の「保存」で今の設定ファイルに上書き保存。まだ保存先が決まっていない（初回）ときは保存先を聞く
  - 「開く...」で好きな設定ファイルを読み込み、「名前を付けて保存...」で保存先を選べる。最後に使ったファイルは次回起動時に自動で開く
  - ファイル選択やメッセージは `IDialogService` 経由で出す（ViewModel から OpenFileDialog / MessageBox を直接使わない）
  - 「元に戻す」で最後に保存した状態に戻る。どちらも変更があるときだけ押せる（保存はエラーがあると押せない）
  - 未保存の変更があるまま終了しようとすると、保存するか確認する
  - 保存する項目は `Settings/AppSettings.cs` に定義（画面と同じ形でなくてよい。例：自動保存の設定は入れ子のクラスでひとまとめ）
  - 画面との対応付けは各ページで手書き：設定 → 画面は `LoadFrom`、画面 → 設定は各プロパティの `OnXxxChanged` で `Write(...)`
  - 項目を増やすときは `[ObservableProperty]`・`OnXxxChanged`・`LoadFrom` の3か所と、`AppSettings` 側に追加する
- 配置を記憶
  - 隠して再表示すると、隠す前の位置に戻る
  - 終了時に `%LocalAppData%\DockSample\layout.xml` へ保存し、次回起動時に復元（ファイルを消すと初期配置に戻る）
- 各ペインの中身は `Views/` の UserControl に分割。`App.xaml` で ViewModel → View を対応付け

## 構成

```
DockSample/
├─ App.xaml(.cs)            DI 設定 / ViewModel→View の DataTemplate
├─ MainWindow.xaml(.cs)     ボタン + DockingManager / レイアウト保存・復元の呼び出し
├─ Views/                   各ペインの UserControl
├─ Controls/                使い回す部品（NumericUpDown）
├─ ViewModels/              Main / Pane(基底) / Document / Explorer, Properties, Output
├─ Settings/                保存する設定のクラス
├─ Docking/                 LayoutInitializer(初期配置) / DockLayoutStore(保存・復元)
├─ Styles/                  文字・コントロールの共通スタイル
├─ Converters/              bool → Visibility(Hidden)
└─ Services/                ILogService / IAppearanceService(文字サイズの変更) / ISettingsService(設定の保存) / IDialogService(ファイル選択・メッセージ)
```

## 実行

```
cd DockSample
dotnet run
```

Windows + .NET 9 SDK が必要（Dirkster.AvalonDock v5 を使用。v5 は net9 / net10 / net48 のみ対応）。
