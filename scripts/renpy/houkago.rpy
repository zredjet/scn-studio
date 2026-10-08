# samples/houkago：選択肢で灯と紬のエンディングに着く。bad_end は始まりから着けないので s030 から始め、
# 行き止まりのスタブ（s014）は、知らせてタイトルへ戻ることを確かめる。

testcase akari_end:
    $ scn_seen.clear()
    run Start()
    advance until "すぐに行く" timeout 60
    click "すぐに行く"
    advance until "ごめん、遅くなって" timeout 60
    click "ごめん、遅くなって"
    advance until eval ("s040" in scn_seen) timeout 60
    advance until eval main_menu timeout 60

testcase tsumugi_end:
    $ scn_seen.clear()
    run Start()
    advance until "少し迷ってから行く" timeout 60
    click "少し迷ってから行く"
    advance until "ごめん、遅くなって" timeout 60
    click "ごめん、遅くなって"
    advance until eval ("s060" in scn_seen) timeout 60
    advance until eval main_menu timeout 60

testcase bad_end:
    $ scn_seen.clear()
    run Start("s030")
    advance until eval ("s061" in scn_seen) timeout 60
    advance until eval main_menu timeout 60

testcase dead_end:
    $ scn_seen.clear()
    run Start("s014")
    advance until "行き止まり" timeout 60
    advance until eval main_menu timeout 60
