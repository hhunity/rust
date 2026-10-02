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
  - どこかのタブにエラーがあると、上部の「設定」ボタンにも同じ印を表示
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
├─ ViewModels/              Main / Pane(基底) / Document / Explorer, Properties, Output
├─ Docking/                 LayoutInitializer(初期配置) / DockLayoutStore(保存・復元)
├─ Converters/              bool → Visibility(Hidden)
└─ Services/                ILogService
```

## 実行

```
cd DockSample
dotnet run
```

Windows + .NET 9 SDK が必要（Dirkster.AvalonDock v5 を使用。v5 は net9 / net10 / net48 のみ対応）。
