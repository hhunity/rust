# WPF AvalonDock MVVM + DI Sample

Dirkster.AvalonDock を MVVM (CommunityToolkit.Mvvm) と DI (Microsoft.Extensions.DependencyInjection) で使うサンプル。

- 上部の水平ボタンで、下のドッキングスペースにペインを開く
  - 「新規ドキュメント」: 押すたびに新しいドキュメントタブ
  - 「エクスプローラー / プロパティ / 出力」: トグルボタン。押すと表示、もう一度押すと非表示
- ペインはドラッグでドッキング位置の変更・フローティングが可能
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
├─ ViewModels/              Main / Document / Tool(Explorer, Properties, Output)
├─ Docking/                 StyleSelector / LayoutInitializer / DockLayoutStore(保存・復元)
├─ Converters/              bool → Visibility(Hidden)
└─ Services/                ILogService
```

## 実行

```
cd DockSample
dotnet run
```

Windows + .NET 9 SDK が必要（Dirkster.AvalonDock v5 を使用。v5 は net9 / net10 / net48 のみ対応）。
