// Show which library and app versions the docs are for, at the top of the sidebar.
// window.openStompVersions comes from versions.js, written by site/assemble.py.
(function () {
    const v = window.openStompVersions;
    const box = document.querySelector(".sidebar-scrollbox");
    if (!v || !box) return;
    const root = typeof path_to_root === "string" ? path_to_root : "";
    const line = (name, info, page) => {
        const a = document.createElement("a");
        a.href = root + page;
        a.textContent = `${name} ${info.version}`;
        if (info.ahead) {
            const note = document.createElement("span");
            note.className = "unreleased";
            note.textContent = " + unreleased";
            a.append(note);
            a.title = `main at ${info.commit}: ${info.ahead} commit${info.ahead === 1 ? "" : "s"} after v${info.version}`;
        }
        return a;
    };
    const div = document.createElement("div");
    div.className = "docs-versions";
    const label = document.createElement("div");
    label.className = "docs-versions-label";
    label.textContent = "Docs for";
    div.append(label, line("open-stomp", v.lib, "lib/CHANGELOG.html"),
        line("Open Stomp Manager", v.manager, "manager/CHANGELOG.html"));
    box.prepend(div);
})();
