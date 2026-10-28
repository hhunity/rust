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
  - 設定は**フォルダ**単位で保存する。中にセクションごとの JSON（`general.json`, `appearance.json`, `items.json`, `waveforms.json`）を作る
    - どのセクションをどのファイルに入れるかは `JsonSettingsService.Sections` の一覧で決める（増やすときは1行足す）
    - 開くとき、フォルダにないファイルのセクションは初期値になる（後からセクションを増やしても古いフォルダを開ける）
  - 設定ペイン下の「保存」で今のフォルダに上書き保存。まだ保存先が決まっていない（初回）ときは保存先フォルダを聞く
  - 「開く...」でフォルダを選んで読み込み、「名前を付けて保存...」で保存先フォルダを選べる。最後に使ったフォルダは次回起動時に自動で開く
  - 既に設定ファイルがあるフォルダに保存するときは上書きを確認。設定ファイルが1つもないフォルダは開けない
  - フォルダ選択やメッセージは `IDialogService` 経由で出す（ViewModel から OpenFolderDialog / MessageBox を直接使わない）
  - 「↶ 元に戻す」(Ctrl+Z) / 「↷ やり直し」(Ctrl+Y) で1つずつ戻す・進める。同じ項目の続けての変更（文字入力など）は1回分にまとまる。戻れるのは最大 50 回（`JsonSettingsService.MaxHistory`。古いものから捨てる）
  - 「保存した状態に戻す」で最後に保存した状態にまとめて戻る（これも Undo で取り消せる）。「開く」で Undo の履歴は消える
  - 未保存の変更があるまま終了しようとすると、保存するか確認する
  - 保存する項目は `Settings/AppSettings.cs` に定義（画面と同じ形でなくてよい。例：自動保存の設定は入れ子のクラスでひとまとめ）
  - 画面との対応付けは各ページで手書き：設定 → 画面は `LoadFrom`、画面 → 設定は各プロパティの `OnXxxChanged` で `Write(...)`
  - 設定の書き換えは全部 `ISettingsService.Edit(key, s => ...)` を通す（Undo の記録 → 書き換え → 「未保存」の更新をまとめて行う。「元に戻す」「開く」で読み直している最中の書き戻しは無視する）
  - 項目を増やすときは `[ObservableProperty]`・`OnXxxChanged`・`LoadFrom` の3か所と、`AppSettings` 側に追加する
- 「一覧」ペイン（`ItemListViewModel` / `Views/ItemListView.xaml`）
  - 「追加 ▾」（DropDownButton）で種類を選ぶと、表（DataGrid）に1行追加
  - 列：名前・種類・備考・有効（チェックボックス）・状態・削除ボタン
    - 状態：「新」= 追加してまだ保存していない / 「●」= 保存したあとに編集した（元の値に戻せば消える）
  - 上に「◯件（未保存：新規 ◯ / 編集 ◯ / 削除 ◯）」を表示。無効にした行は文字が薄くなる
  - 行（詳細設定を含む）は設定の一部として、設定フォルダの `items.json` に保存され、次回起動時に復元される
    - 保存データは `Settings/ItemSettings.cs`（`ItemData` と、分類ごとの詳細 `SensorData` など）
    - 詳細設定は型が分類ごとに違うので、JSON に `"kind": "sensor"` のような目印を付けて保存（`[JsonPolymorphic]` / `[JsonDerivedType]`）
    - 追加・削除・編集のたびに全行を設定に書き写すので、設定ペインの「保存」「保存した状態に戻す」、終了時の確認がそのまま効く
    - 「↶」「↷」(Ctrl+Z / Ctrl+Y) で元に戻す・やり直し（設定ペインと同じ履歴）
  - 表の右に、選んだ行の詳細パネル（間のつまみで幅を変更可）。名前・備考・有効と、種類の分類ごとの詳細設定を編集できる
    - 詳細設定は分類ごとにクラスが違う（`ViewModels/ItemDetailsViewModels.cs`：センサ / アクチュエータ / 通信 / その他）
    - 表示は型ごとの DataTemplate（`ItemListView.xaml` の Resources）で自動で切り替わる
    - 詳細設定を変えた場合も、その行の「状態」に ● が付く
    - 項目を増やすときは `[ObservableProperty]`・`ToData`・`Load` と、`ItemSettings.cs` の record に追加する
- 「グラフ」ペイン（`GraphViewModel` / `Views/GraphView.xaml`、ScottPlot.WPF 5.x）
  - 波形は設定の一部（`AppSettings.Waveforms` → 設定フォルダの `waveforms.json`）。保存・開く・元に戻すがそのまま効く
    - 形：`"waveforms"` → デバイス名 → uuid → `pulseCollection` → `segmentCollection`（`Settings/WaveformSettings.cs`）
    - どの機種の中身にも表示名 `name`（`DeviceData.Name`）があり、グラフペインの一覧には「デバイス名 / 表示名」で出る（空なら uuid）
    - 中身の型はデバイス名で決まる（`DeviceTypes` の表。表にない名前は `UnknownDeviceData` として中身をそのまま保持）
    - デバイス名ごとに型を選んで読み書きするのは `DeviceTableConverter`
    - クラスに書いていない項目も `[JsonExtensionData]` で保持し、保存時にそのまま書き戻す
    - 既存のファイルは、設定フォルダに `waveforms.json` という名前で置けば読み込める
  - デバイス / uuid を選ぶと、その中身を1つのグラフに描く（パルスを持つ機種なら、パルスを順番につなげた1本の波形になる）
    - データ → 線（`Series`）の変換は機種ごとのクラス（`Graphs/SeriesConverters.cs`、DI に `ISeriesConverter` として登録）
    - 機種を増やすときは：`DeviceData` の派生クラス・`DeviceTypes` の表・変換クラスと DI の登録。グラフ側は変えなくてよい
    - ViewModel は描く線だけを持ち、描くのは View のコードビハインド
  - 上の「追加 ▾」で機種を選ぶと、その機種の波形（新しい uuid）を1つ追加。「削除」で選んでいる波形を削除。「名前」で表示名を変更
  - 波形は点同士を線でつなぎ、点は丸で表示（オレンジ＝動かせる点、灰色＝動かせない点（始まりの (0, 0) など））。各点から横軸・縦軸へ点線の補助線を引く
  - グラフ上の丸（動かせる点）をマウスでつまんで動かせる。どの点を・どの方向に・どの刻みで動かせるかは機種ごとの定義（`Graphs/SeriesConverters.cs` の `GetHandles` / `Move`）が決める
    - パルスを持つ機種：最初の (0, 0) 以外の全部の点（傾斜の終わり・セグメントの終わり）。縦横とも 0.1 刻み（横は 0.1µs）。動くのはその点だけ（前後のセグメントのスルーレート・時間を計算し直す）
    - 電圧源：線の真ん中の点を縦にだけ（0.1 刻み）
    - ドラッグ中は丸だけ動き、離したときに1回だけ書き込む（Undo はドラッグ1回で1回分。表示範囲もそのまま）
  - グラフの右上の「＋」「－」で拡大・縮小、「全体」で全体表示（マウスのホイール・ドラッグでも操作できる）
  - 「エクスポート...」で選んでいる波形1つを別の JSON ファイルに書き出し、「インポート...」で JSON ファイルの波形を追加する
    - ファイルの形は waveforms.json の `"waveforms"` の中身と同じ（`{ "デバイス名": { "uuid": { ... } } }`、中身が1つだけ）。デバイス名も入るので、読み込むときに型が決まる。waveforms.json そのものもインポートできる
    - インポートで同じ機種に同じ uuid があれば、新しい uuid を付けて別のものとして足す。インポートも元に戻すで取り消せる
  - 下の編集欄（`ViewModels/WaveformEditors.cs`）で中身を編集。機種ごとに編集欄の ViewModel と DataTemplate（`GraphView.xaml`）がある
    - 単位：ファイルの時間（duration）は ns、スルーレートは V/µs。グラフと表では時間を µs で見せる（換算は `TimeUnit`）
    - パルスを持つ機種：左にパルスの一覧（先頭に追加・最後に追加・コピー・削除。コピーはすぐ下に複製）。パルスもセグメントも必ず1つ以上（最後の1つは削除ボタンが押せない。新しいパルス・新しい波形はセグメントを1つ持った状態で作る）。パルスの名前は並び順で Pulse0, Pulse1, ... と自動で付き、追加・コピー・削除のたびに振り直す（画面からは変えられない）、右に選んだパルスの名前とセグメントの表（各行の「↑追加」「↓追加」でその行の前・後ろに追加、「＋ 最後に追加」、削除、値の編集）。数値のセルは編集中にマウスホイールで増減（ホイールで 1、Ctrl＋ホイールで 0.1。`Controls/NumericWheel`）
    - 電圧源：電圧と電流制限
    - 編集欄は値を自分で持たず、get / set で毎回 `Current` を読み書きする（`GraphViewModel.Data<T>()` / `Write<T>()`）
    - 追加・削除・編集はどれも元に戻すで取り消せる。グラフも編集のたびに描き直す
    - 選んだものはデータそのものではなく場所（デバイス名・uuid）で覚え、使うたびに `Current` から探し直す
    - セグメント → 点 の変換（`WaveformSeriesConverter.AddSegment`）は仮の解釈（0V 開始、SlewRate の傾きで変化し Duration まで保持。次のパルスは前のパルスの終わりの時刻・電圧から続ける）
- 押すと一覧が開くボタンの部品（`Controls/DropDownButton`）
  - 一覧は `MaxDropDownHeight`（既定 250）を超えるとスクロール。検索欄で絞り込み（`IsSearchEnabled="False"` で非表示）
  - ↑↓で移動・Enter で決定・Esc で閉じる。選んだ項目は `Command` の引数として渡される
  - 一覧を出す方向は `DropDownPlacement="Bottom|Top|Right|Left"`（既定は下）。画面に入りきらなければ自動で反対側に出る
  - 使い方: `<controls:DropDownButton Header="定型文" ItemsSource="{Binding Snippets}" DisplayMemberPath="Name" Command="{Binding InsertSnippetCommand}" Style="{StaticResource Input.DropDown}" />`
  - 例：ドキュメントの「定型文 ▾」で、選んだ定型文を末尾に追加する
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
├─ Controls/                使い回す部品（NumericUpDown / DropDownButton）
├─ ViewModels/              Main / Pane(基底) / Document / Explorer, Properties, Output
├─ Settings/                保存する設定のクラス
├─ Graphs/                  機種のデータ → グラフの線 の変換
├─ Docking/                 LayoutInitializer(初期配置) / DockLayoutStore(保存・復元)
├─ Styles/                  文字・コントロールの共通スタイル
├─ Converters/              bool → Visibility(Hidden)
└─ Services/                ILogService / IAppearanceService(文字サイズの変更) / ISettingsService(設定の保存) / IDialogService(フォルダ選択・メッセージ)
```

## 実行

```
cd DockSample
dotnet run
```

Windows + .NET 9 SDK が必要（Dirkster.AvalonDock v5 を使用。v5 は net9 / net10 / net48 のみ対応）。
