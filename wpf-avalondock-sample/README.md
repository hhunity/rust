# WPF AvalonDock MVVM + DI Sample

Dirkster.AvalonDock を MVVM (CommunityToolkit.Mvvm) と DI (Microsoft.Extensions.DependencyInjection) で使うサンプル。

- 上部の水平ボタンを押すと、下のドッキングスペースにペインが開く
  - 「新規ドキュメント」: 押すたびに新しいドキュメントタブ
  - 「エクスプローラー / プロパティ / 出力」: 単一インスタンスのツール（左 / 右 / 下に配置）
- ペインはドラッグでドッキング位置の変更・フローティングが可能

## 実行

```
cd DockSample
dotnet run
```

Windows + .NET 8 SDK が必要。
