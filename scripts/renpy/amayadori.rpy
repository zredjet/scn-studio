# samples/amayadori：選択肢だけで、3つのエンディングすべてに着き、タイトルへ戻る。

testcase rain_end:
    $ scn_seen.clear()
    run Start()
    advance until "本を開く" timeout 60
    click "本を開く"
    advance until "黙って待つ" timeout 60
    click "黙って待つ"
    advance until eval ("s11" in scn_seen) timeout 60
    advance until eval main_menu timeout 60

testcase bus_end:
    $ scn_seen.clear()
    run Start()
    advance until "話しかける" timeout 60
    click "話しかける"
    advance until "傘を差し出す" timeout 60
    click "傘を差し出す"
    advance until eval ("s09" in scn_seen) timeout 60
    advance until eval main_menu timeout 60

testcase stay_end:
    $ scn_seen.clear()
    run Start()
    advance until "話しかける" timeout 60
    click "話しかける"
    advance until "行かなくていいんじゃないか" timeout 60
    click "行かなくていいんじゃないか"
    advance until eval ("s10" in scn_seen) timeout 60
    advance until eval main_menu timeout 60
