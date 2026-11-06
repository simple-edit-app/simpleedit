use iced::{
    widget::{button, column, container, row, scrollable, text, text_editor, text_input, Space},
    Alignment, Element, Length, Task, Theme,
};
use std::collections::{HashMap, HashSet};

use crate::history::{History, Snapshot};
use std::path::{Path, PathBuf};

use crate::{
    config::{Config, ShortcutConfig, ShortcutTarget},
    editor::EditorState,
    preferences::{PreferencesMessage, PreferencesState},
    preview::{self, PreviewKind, ViewMode},
    search::{SearchMessage, SearchState},
    sidebar::{SidebarAction, SidebarMessage, SidebarState},
    tabs::{fingerprint, CachedBuffer, DiskStamp, Loaded, TextFormat},
    text_ops::{self, comment_style},
    theme as se_theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopMenu {
    File,
    Edit,
    View,
    Help,
}

/// The places a name is used, with the highlighted one (arrows and Enter).
#[derive(Debug, Clone, PartialEq)]
pub struct RefList {
    pub word: String,
    pub items: Vec<crate::lsp::Reference>,
    pub selected: usize,
    pub state: RefState,
}

/// What the usages popup shows: it opens as soon as the search starts.
#[derive(Debug, Clone, PartialEq)]
pub enum RefState {
    Searching,
    /// The search ended with nothing to list: why.
    Empty(String),
    Ready,
}

impl RefList {
    pub(crate) fn searching(word: String) -> Self {
        Self {
            word,
            items: Vec::new(),
            selected: 0,
            state: RefState::Searching,
        }
    }
}

/// The language servers that can be run, and the ones that can be removed.
pub type Servers = (
    std::collections::HashSet<&'static str>,
    std::collections::HashSet<&'static str>,
);

/// What the Git shortcuts ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitShortcut {
    /// The Commit tab, to write the message.
    Commit,
    Push,
    Pull,
}

/// What the arrows of a diff tab do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffNav {
    PrevChange,
    NextChange,
    PrevFile,
    NextFile,
    ToggleSide,
    /// The mouse on the rows of the diff.
    Pick(crate::git_panel::DiffPointer),
    /// The diff was scrolled: (offset, height shown).
    Scrolled(i32, i32),
    CopyPicked,
    PickAll,
    /// Put the old version of a block of changes in the file.
    Apply(usize),
    /// Stage (or unstage) a block, or the lines picked; blanks left out of
    /// the diff; more or less context.
    StageBlock(usize),
    StageSelection,
    ToggleBlanks,
    CycleContext,
    /// A commit's diff against the current file, and back.
    ToggleCompare,
    /// Opens the file of the diff in the editor.
    OpenFile,
}

/// A conflicted file open in an editor tab (`merge://N/path`).
pub type MergeTabs = HashMap<PathBuf, crate::git_panel::merge::MergeTab>;

/// A diff shown in an editor tab (`diff://N/title`): the repository, the
/// files to walk through, where it is.
pub struct DiffTabState {
    pub repo: PathBuf,
    pub viewer: crate::git_panel::Viewer,
}

/// The blame column of a file: what is drawn, and the commit of each line.
pub struct BlameState {
    pub path: PathBuf,
    pub repo: PathBuf,
    /// The path inside the repository.
    pub relative: String,
    pub annotations: crate::code_editor::Annotations,
    pub shas: Vec<String>,
}

/// What the current file is compared with to mark its changed lines.
pub struct MarksState {
    pub path: PathBuf,
    /// What HEAD has of it (`None`: git doesn't know the file yet).
    pub base: Option<String>,
    pub marks: crate::git::marks::ChangeMarks,
    /// The marks as the editor takes them, shared between frames.
    pub paint: [std::sync::Arc<Vec<usize>>; 3],
    /// The buffer (see [`SimpleEditApp::buffer_stamp`]) the marks were computed for.
    pub rev: (u64, u64),
}

/// Which modal overlay (if any) is currently shown.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveOverlay {
    None,
    /// Search everywhere (Shift twice).
    Everywhere,
    About,
    /// Every release.
    Changelog,
    /// The releases since the given version, shown after an update.
    WhatsNew(String),
    GotoLine,
    LanguagePicker,
    /// Where a name is used: the list to jump from.
    References(RefList),
    Shortcuts,
    /// The files of an operation that are in conflict.
    Conflicts,
    UpdateChecking,
    UpdateUpToDate,
    /// The new version, and what it changes.
    UpdateAvailable(String, Vec<crate::changelog::Entry>),
    UpdateDownloading,
    UpdateInstalling,
    UpdateError(String),
    /// Closing this tab would discard unsaved changes.
    ConfirmClose(PathBuf),
    /// Closing these tabs would discard unsaved changes in some of them.
    ConfirmCloseMany(Vec<PathBuf>, usize),
    /// The session couldn't be written: quitting would lose unsaved buffers.
    ConfirmQuit(String),
    /// Saving would overwrite changes made to the file outside the editor.
    ConfirmOverwrite(PathBuf),
    /// A scratch file is about to be deleted for good.
    ConfirmScratchDelete(PathBuf),
    /// A text looked for in the files of the projects.
    FindInFiles,
    /// A name asked for a file or folder of a project.
    ProjectName(project_ops::NameDialog),
    /// A folder to delete for good, everything in it.
    ConfirmFolderDelete(PathBuf),
    /// A file or folder dragged onto a folder, to move there.
    ConfirmMove(PathBuf, PathBuf),
    /// A file to delete for good; `Some(on)`: git tracks it, and the box
    /// "remove it from Git too" is (un)checked.
    ConfirmFileDelete(PathBuf, Option<bool>),
    /// The list that switches branch from the status bar, with what is typed.
    BranchSwitch(String),
    /// The Git actions of a file.
    GitFile(PathBuf),
    /// A branch, tag or revision to compare a file with.
    GitPick(Box<git_file::Pick>),
}

#[derive(Debug, Clone)]
pub enum Message {
    EditorAction(text_editor::Action),
    Sidebar(SidebarMessage),
    Search(SearchMessage),
    Preferences(PreferencesMessage),
    OpenFile,
    /// A file chosen by the user (dialog, command line, recent files).
    FileOpened(Result<Loaded, String>),
    /// A file read to switch to its tab; superseded by a later switch.
    TabRead(Result<Loaded, String>),
    /// The open file changed on disk and was read again.
    FileReloaded(Result<Loaded, String>),
    SaveFile,
    SaveFileAs,
    /// The path picked in "Save as", with the tab it was started from.
    SaveAsChosen(Option<PathBuf>, Result<PathBuf, String>),
    /// The tab the save was started from, and the path + exact text written.
    FileSaved(Option<PathBuf>, Result<(PathBuf, String), String>),
    NewFile,
    CloseFile,
    /// Closes every tab but this one (none: all of them).
    CloseTabs(Option<PathBuf>),
    /// Closes every tab but the active one.
    CloseOtherTabs,
    /// The user confirmed closing these tabs although some have unsaved changes.
    ConfirmDiscardMany(Vec<PathBuf>),
    ToggleSearch,
    ToggleSidebar,
    /// A tab was dragged from a place to another.
    TabMoved(usize, usize),
    /// A click on a file of the tree: it opens in the preview tab (a second
    /// click, at once, keeps it open).
    TreeOpenFile(PathBuf),
    /// The preview tab becomes a tab like the others.
    KeepTab,
    /// Two clicks on a tab (the preview one stays then).
    TabDoubleClick(usize),
    /// The middle button closed the tab at this place.
    CloseTabAt(usize),
    /// Opens again the tab closed last.
    ReopenTab,
    /// Asks for a file of the projects by its name.
    OpenGotoFile,
    /// Looks for a text in the files of the projects.
    OpenFindInFiles,
    /// The files of the projects, listed in the background.
    FinderFiles(Vec<(PathBuf, String)>),
    FindQuery(String),
    FindToggleCase,
    FindToggleRegex,
    /// Replace in files, and the rest of the window: the replacement, whole
    /// words, keeping the case, find or replace, the context, the file mask,
    /// where, subfolders, a list under the fields, a past search, the pin, a
    /// match picked, replacing one or all (and the confirmation), undoing,
    /// the matches to the Results panel.
    OpenReplaceInFiles,
    ReplaceInFolder(PathBuf),
    FindReplaceText(String),
    FindToggleWords,
    FindToggleMultiline,
    FindIgnored(bool),
    FindLines(text_editor::Action),
    FindTogglePreserve,
    FindMode(bool),
    FindContext(crate::find_replace::Context),
    FindMaskOn(bool),
    FindMask(String),
    FindScope(finder::Scope),
    FindRecursive,
    FindMenu(Option<finder::Menu>),
    FindHistory(String),
    FindPin,
    FindSelect(usize),
    FindReplaceOne,
    FindReplaceAll,
    FindReplaceConfirm(bool),
    FindUndoReplace,
    FindToWindow,
    Results(results::ResultsMsg),
    /// What a search found (its generation, the places or what went wrong).
    FindResults(u64, Result<Vec<finder::Occurrence>, String>),
    FindOpen(Option<usize>),
    /// Looks in this folder only (from the tree's menu).
    FindInFolder(PathBuf),
    /// The list of every open tab, opened from its button (where it was pressed).
    OpenTabList(iced::Point),
    CloseTabList,
    /// A tab picked in that list.
    PickTab(PathBuf),
    /// Opens or closes a panel of the sidebar from the bar on its left.
    TogglePanel(crate::sidebar::Panel),
    /// A setting of the lint, changed in the Preferences.
    Lint(crate::lint::Setting),
    /// A run of the linters is done (its generation, the text it read).
    LintDone(u64, lint::Key, lint::Outcome),
    /// The pointer rests on a problem (and where its card goes), or left it.
    LintHover(Option<(usize, iced::Point)>),
    /// Completion: the shortcut, what a server told about it, its answer,
    /// where the list hangs in the window, a click on an item, the wheel
    /// over the list, an item's documentation, the import an accepted item
    /// needs, whether the server still loads the project.
    CompletionInvoke,
    CompletionInfo(
        (String, PathBuf),
        Result<crate::lsp::CompletionInfo, String>,
    ),
    CompletionAnswer(
        u64,
        Box<(
            crate::completion::Base,
            Result<crate::lsp::Completions, String>,
        )>,
    ),
    CompletionAnchor(crate::code_editor::Spot, crate::code_editor::Spot),
    /// The help on the parameters of a call: asked with the shortcut, its
    /// answer, the next or previous signature of an overloaded function.
    HintInvoke,
    HintAnswer(u64, Result<Option<serde_json::Value>, String>),
    CompletionPick(usize),
    CompletionWheel(f32),
    CompletionResolved(u64, usize, Result<serde_json::Value, String>),
    CompletionImport(u64, Result<serde_json::Value, String>),
    CompletionBusy(u64, bool),
    LintCardHover(bool),
    LintCloseCheck(u64),
    /// The fixes a language server has for a problem.
    LintFixesLoaded(lint::Key, usize, Result<Vec<crate::lint::Fix>, String>),
    /// Applies a fix (of a problem, by number) to the text.
    LintApply(usize, usize),
    LintFixAll,
    /// Fixes the problems of one source (a linter, a server) only.
    LintFixSource(String),
    LintFixedAll(lint::Key, String),
    LintFixFile(PathBuf),
    /// Looks at the file now.
    LintNow,
    /// Opens the Preferences on the Lint page.
    OpenLintSettings,
    /// Asks for a folder to add to the sidebar as a project.
    AddProject,
    ProjectPicked(Option<PathBuf>),
    /// Opens or closes a folder of the tree.
    ToggleProjectFolder(PathBuf),
    /// Takes a project out of the sidebar (nothing is deleted).
    RemoveProject(PathBuf),
    RefreshProjects,
    /// How the files of the repositories stand (paths, states, folder or not).
    FileStates(Vec<(PathBuf, crate::git::FileState, bool)>),
    /// Opens the folders of the tree down to a file.
    ShowInProjects(PathBuf),
    /// Closes every folder of the trees.
    CollapseProjects,
    /// A line of the tree pressed (it is picked, and may be dragged), the
    /// pointer over another line, the button let go.
    TreePress(PathBuf),
    TreeHover(PathBuf),
    TreeRelease,
    /// Moves a file or folder into a folder (after the question).
    MoveEntry(PathBuf, PathBuf),
    /// The tree was scrolled: how far, and how high it is.
    TreeScrolled(f32, f32),
    /// The names left out of the trees (as typed: separated by commas).
    SetProjectsExclude(String),
    /// A terminal that starts in this folder.
    OpenTerminalAt(PathBuf),
    /// Shows a file or folder in the system's file manager.
    RevealInFileManager(PathBuf),
    /// Asks a name for a copy of a file or folder.
    DuplicateEntry(PathBuf),
    /// Asks a name for a file (false) or a folder (true) in a folder.
    NewProjectEntry(PathBuf, bool),
    RenameProjectEntry(PathBuf),
    ProjectNameChanged(String),
    ProjectNameSubmit,
    AskDeleteFolder(PathBuf),
    DeleteFolder(PathBuf),
    FolderDeleted(Result<PathBuf, String>),
    /// Puts a path on the clipboard.
    CopyPath(PathBuf),
    /// What Git ignores is left out of the trees (or shown again).
    ToggleHideIgnored,
    /// The cursor goes to the next problem (false: the previous one).
    LintNext(bool),
    /// Splits the editor in two (side by side, or one above the other).
    Split(bool),
    /// Shows a file in a pane of the split (the editor is split first).
    SplitWith(PathBuf, bool),
    /// Closes the active pane.
    CloseSplit,
    ClosePane(split::PaneId),
    /// A pane takes the focus.
    SplitFocus(split::PaneId),
    /// Something done in a pane that has not the focus.
    SplitAction(split::PaneId, text_editor::Action),
    /// The edge of the file list is grabbed, dragged (x of the pointer) and let go.
    /// The way a pull / a merge is made, chosen in the Git panel or the Preferences.
    SetPullMode(crate::git::PullMode),
    /// Minutes between two fetches in the background (0: never).
    SetAutoFetch(u32),
    /// The branch switcher of the status bar: open, typed in, chosen (the
    /// name, and whether it is made first).
    OpenBranchSwitcher,
    BranchSwitchFilter(String),
    BranchSwitchChoose(String, bool),
    SetMergeMode(crate::git::MergeMode),
    /// A Git shortcut: opens the panel and does it.
    GitShortcut(GitShortcut),
    SidebarResizeStart,
    SidebarResize(f32),
    SidebarResizeEnd,
    PanelResizeStart,
    PanelResize(f32),
    PanelResizeEnd,
    /// Delete a file for good: ask first, then do it (from Git as well when tracked).
    AskDeleteFile(PathBuf),
    /// Whether git tracks the file asked about (it decides on the checkbox).
    DeleteAsked(PathBuf, bool),
    ToggleDeleteFromGit(bool),
    /// Delete the file; `true`: from git as well.
    DeleteFile(PathBuf, bool),
    FileDeleted(Result<bool, String>),
    TogglePreferences,
    ThemeChanged(bool),
    /// Switches between the dark and the light theme.
    ToggleTheme,
    InsertTab,
    ToggleMenu(TopMenu),
    /// A click outside the open menu: it closes (nothing else happens).
    CloseMenu,
    // Git
    ToggleGit,
    Git(crate::git_panel::GitMsg),
    /// A diff to show in an editor tab.
    OpenDiffTab(crate::git_panel::DiffTab),
    DiffTab(DiffNav),
    // Conflicts
    OpenConflicts,
    OpenMergeTab(std::sync::Arc<crate::git_panel::MergeSpec>),
    Merge(crate::git_panel::merge::MergeMsg),
    MergeApplied(PathBuf, Result<(), String>),
    /// A change was moved from one side of a diff to the other.
    DiffApplied(Result<(), String>),
    // Blame
    BlameFile,
    BlameLoaded(
        PathBuf,
        Result<(PathBuf, String, Vec<crate::git::history::BlameLine>), String>,
    ),
    CloseBlame,
    BlameLineClicked(usize),
    /// Shows the file as it was before the commit of the line that was
    /// right-clicked in the blame column, with its own blame.
    BlamePrevious,
    BlamePreviousLoaded(
        Result<
            (
                PathBuf,
                String,
                String,
                String,
                Vec<crate::git::history::BlameLine>,
            ),
            String,
        >,
    ),
    /// The bar of a changed line was clicked: the button that puts the change
    /// back opens under it (or closes).
    ChangeBar(usize),
    RevertChange,
    CloseRevert,
    /// Closes the pop-up that announces conflicts.
    CloseConflictToast,
    /// What HEAD has of a file, for its changed-line marks.
    MarksBase(PathBuf, Option<Option<String>>),
    // Terminal
    ToggleTerminal,
    NewTerminal,
    // Scratch files
    ToggleScratch,
    // The Git menu of a file
    GitFileMenu(PathBuf),
    GitFileDo(PathBuf, git_file::FileAction),
    GitFileRoot(PathBuf, git_file::FileAction, Option<PathBuf>),
    GitFileDone(Result<String, String>),
    GitFileDiff(Result<crate::git_panel::DiffTab, String>),
    GitFilePick(Result<git_file::Pick, String>),
    GitPickFilter(String),
    GitPickChoose(String),
    /// The revision typed in the picker exists: (repository, file, revision).
    GitPickVerified(Result<(PathBuf, String, String), String>),
    // Bookmarks panel
    ToggleBookmarkPanel,
    BookmarkClick(bookmark_panel::Target),
    BookmarkFold(PathBuf),
    BookmarkDelete(bookmark_panel::Target),
    /// Bookmarks a file as a whole, or takes it out.
    ToggleFileBookmark(PathBuf),
    ScratchName(String),
    ScratchExt(crate::scratch::Ext),
    ScratchCreate,
    ScratchOpen(PathBuf),
    ScratchAskDelete(PathBuf),
    ScratchDelete(PathBuf),
    PickScratchDir,
    ScratchDirChosen(Option<PathBuf>),
    /// Keeps the scratch files in this folder ("" for the default one).
    SetScratchDir(String),
    SelectTerminal(usize),
    CloseTerminal(usize),
    /// Time to repaint the terminal and drop the ones whose shell ended.
    TerminalTick,
    TerminalFocus(bool),
    TerminalCopy(String),
    TerminalPaste,
    TerminalPasteText(Option<String>),
    /// The pointer entered a menu title: switches menus if one is already open.
    HoverMenu(TopMenu),
    /// Resolves the on-screen position of an open top menu's button, so its
    /// dropdown can be anchored under it regardless of label/font/DPI.
    MenuBounds(TopMenu, Option<iced::Rectangle>),
    SelectAll,
    Quit,
    About,
    ContextCopy,
    ContextCut,
    ContextPaste,
    ContextDelete,
    ClipboardContent(Option<String>),
    FormatFile,
    FormatSelection,
    FileFormatted(FormatJob, Result<String, String>),
    SelectionFormatted(FormatJob, Result<String, String>),
    CloseErrorPanel,
    Undo,
    Redo,
    AutoSave,
    // Raw key press forwarded from the subscription (non-capturing fn pointer)
    KeyPressed {
        key: String,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
    EscapePressed,
    /// Enter pressed while a search field might own it; `shift` goes back.
    SearchEnter {
        shift: bool,
    },
    SearchEnterChecked {
        shift: bool,
        in_find_field: bool,
    },
    /// The autosave wrote (or failed to write) the session.
    SessionWritten(Result<(), String>),
    /// Quit although the session couldn't be saved.
    QuitAnyway,
    /// Re-parses the preview if the text changed (typing is debounced).
    PreviewTick,
    /// The window was resized (kept for the next start).
    WindowResized(f32, f32),
    // Overlays
    CloseOverlay,
    OpenGotoLine,
    GotoLineInputChanged(String),
    GotoLineSubmit,
    OpenLanguagePicker,
    SetLanguage(String),
    OpenShortcuts,
    StartCaptureShortcut(ShortcutTarget),
    // Task palette
    OpenPalette,
    /// Search everywhere: opened on a tab, Shift twice, what is typed,
    /// another tab, a line opened, what the searches found, the options.
    OpenEverywhere(crate::everywhere::Tab),
    DoubleShift,
    EverywhereQuery(String),
    EverywhereTab(crate::everywhere::Tab),
    EverywhereOpen(Option<usize>),
    EverywhereFiles(u64, Vec<(PathBuf, String)>),
    EverywhereSymbolsDue(u64),
    EverywhereSymbols(u64, crate::lsp::Symbols),
    EverywhereTextDue(u64),
    EverywhereText(u64, Result<Vec<crate::finder::TextHit>, String>),
    EverywhereGit(Vec<String>, Vec<Commit>),
    EverywhereNonProject,
    EverywhereFilter,
    EverywhereFilterType(String, bool),
    EverywhereTextOption(u8),
    EverywhereExport,
    EverywhereAbbreviate(crate::palette::PaletteCommand),
    EverywhereAbbreviationTyped(String),
    EverywhereAbbreviationSubmit,
    EverywhereAbbreviationRemove(crate::palette::PaletteCommand, String),
    EverywhereAssign(crate::palette::PaletteCommand),
    // Line-level editing
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    DeleteLine,
    // Bookmarks
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    ClearBookmarks,
    NavBack,
    NavForward,
    ToggleFold,
    FoldAll,
    UnfoldAll,
    /// A click on a chevron of the gutter (line of the block, how far it reaches).
    FoldClick(usize, crate::folding::Scope),
    /// Ctrl/Cmd+click on a name: look for where it is defined.
    GotoDefinition,
    /// A line of the list of references was chosen.
    OpenReference(usize),
    /// Installs a language server (its program name) from the Preferences.
    InstallServer(&'static str),
    UninstallServer(&'static str),
    ServerUninstalled(&'static str, Result<(), String>),
    ServerInstalled(&'static str, Result<(), String>),
    /// The language servers that can be run.
    ServersChecked(Servers),
    /// The name looked for, and where it is defined, if anywhere.
    DefinitionFound(String, crate::lsp::Outcome),
    /// A right click over the text, relative to the editor.
    EditorRightClick(iced::Point),
    IndentSelection,
    DedentLine,
    // Recent files
    OpenRecentFile(PathBuf),
    SetRecents(bool),
    ClearRecents,
    /// Whether a search field had focus when an editor shortcut was pressed.
    ShortcutChecked(Box<Message>, bool),
    /// The user confirmed closing a tab that has unsaved changes.
    ConfirmDiscard(PathBuf),
    /// The user chose to overwrite a file that changed on disk.
    ConfirmOverwrite,
    // Updater
    CheckForUpdate,
    UpdateChecked(Result<Option<(String, Vec<crate::changelog::Entry>)>, String>),
    OpenChangelog,
    ConfirmInstallUpdate(String),
    UpdateDownloaded(Result<(std::path::PathBuf, String), String>),
    UpdateInstalled(Result<(), String>),
    // Markdown / JSON preview
    SetViewMode(ViewMode),
    JsonToggle(String),
    JsonExpandAll,
    JsonCollapseAll,
    /// Live modifier state, so a chevron click can tell Ctrl+click from Alt+click.
    ModifiersChanged {
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// An in-document anchor link (`#slug`) was clicked in the Markdown
    /// preview.
    MarkdownLinkClicked(String),
    /// Resolves the position of the heading `MarkdownLinkClicked` targeted.
    PreviewScrollTo(Option<f32>),
}

/// What the window starts with.
#[derive(Default)]
pub struct Flags {
    pub config: Config,
    /// Files named on the command line.
    pub files: Vec<PathBuf>,
}

pub struct SimpleEditApp {
    config: Config,
    /// Marked lines of every file, saved between runs.
    bookmarks: crate::bookmarks::Bookmarks,
    editor: EditorState,
    /// How the files stand (Git, lint), for their colors.
    file_marks: crate::projects::Marks,
    /// What "go to file" and "find in files" look into and found.
    finder: finder::FinderState,
    /// The folders added as projects, shown as trees in the sidebar.
    projects: crate::projects::Projects,
    lint: lint::LintState,
    completion: completion::CompletionState,
    /// The editor split in two panes, if it is.
    split: Option<split::Split>,
    sidebar: SidebarState,
    search: SearchState,
    preferences: PreferencesState,
    show_search: bool,
    show_sidebar: bool,
    /// The open files from the one seen last, and, while Ctrl+Tab is held,
    /// that order frozen and how far into it the switch went.
    mru: Vec<PathBuf>,
    tab_switch: Option<(Vec<PathBuf>, usize)>,
    /// The tab a file of the tree opened in, to be replaced by the next one
    /// (it is kept once it is edited or opened twice), the one it replaces
    /// with its place, and the last click in the tree.
    preview_tab: Option<PathBuf>,
    preview_replaces: Option<(PathBuf, usize)>,
    tree_click: Option<(PathBuf, std::time::Instant)>,
    /// The arrows move in the tree (it was clicked last).
    tree_focus: bool,
    /// The line pressed in the tree, and whether it was dragged since.
    tree_drag: Option<(PathBuf, bool)>,
    /// How far the tree is scrolled and how high it is.
    tree_view: (f32, f32),
    /// The tabs closed, last one last (Ctrl+Shift+T opens them again).
    closed_tabs: Vec<PathBuf>,
    /// The list of every tab is open, from where its button was pressed.
    tab_list: Option<iced::Point>,
    /// The panels of the sidebar that are open (the bar on its left).
    panels: crate::sidebar::Panels,
    /// Width of the file list, and whether its edge is being dragged.
    sidebar_width: f32,
    sidebar_resizing: bool,
    /// The height of the bottom panels (Git, terminal, scratch files, bookmarks), and the drag that resizes it: where the
    /// pointer and the height were at its first move.
    panel_height: f32,
    panel_resizing: bool,
    panel_drag: Option<(f32, f32)>,
    show_preferences: bool,
    /// The shortcut being rebound, while waiting for its new key.
    capturing: Option<ShortcutTarget>,
    everywhere: everywhere::Everywhere,
    results: results::Results,
    open_menu: Option<TopMenu>,
    /// The recent files submenu of the File menu is showing.
    recents_open: bool,
    /// Last known screen position of each menu button. Reused while a fresh
    /// query is in flight so switching menus doesn't flicker.
    menu_bounds: HashMap<TopMenu, iced::Rectangle>,
    current_file: Option<PathBuf>,
    is_dirty: bool,
    is_formatting: bool,
    status_message: String,
    terminal: crate::terminal::Panel,
    scratch: crate::scratch::Panel,
    bookmark_panel: bookmark_panel::Panel,
    git: crate::git_panel::Panel,
    diff_tabs: HashMap<PathBuf, DiffTabState>,
    diff_counter: u32,
    /// What files held before a rollback or an arrow of a diff changed them
    /// (Ctrl+Z in a diff brings it back), and what Ctrl+Y puts again.
    work_undo: Vec<(PathBuf, String)>,
    work_redo: Vec<(PathBuf, String)>,
    merge_tabs: MergeTabs,
    merge_counter: u32,
    blame: Option<BlameState>,
    marks: Option<MarksState>,
    /// The file the marks were last asked for.
    marks_asked: Option<PathBuf>,
    last_edit: std::time::Instant,
    /// A save by itself is under way (it says nothing in the status bar).
    autosaving: bool,
    ticks: u32,
    /// The last right click on the editor was in the gutter.
    gutter_click: bool,
    /// The line of the gutter that was right-clicked (for the blame menu).
    gutter_line: Option<usize>,
    /// The change whose revert button is open: file, line, and the buffer it was
    /// opened on (typing closes it).
    revert: Option<(PathBuf, usize, (u64, u64))>,
    /// The pop-up that announces conflicts was closed by the user (until
    /// the conflicts are gone and come back).
    conflict_toast_closed: bool,
    /// A language server to offer in the status bar, in place of the message.
    server_offer: Option<&'static crate::lsp_install::ServerInfo>,
    status_is_error: bool,
    format_error: Option<String>,
    show_error_panel: bool,
    untitled_counter: u32,
    file_cache: HashMap<PathBuf, CachedBuffer>,
    overlay: ActiveOverlay,
    goto_line_input: String,
    /// Word-level undo history (capped by count and total size).
    undo_stack: History,
    /// Redo history rebuilt when undoing.
    redo_stack: History,
    /// Undo/redo history of the tabs that aren't the active one.
    undo_cache: HashMap<PathBuf, (History, History)>,
    /// Preference changes are written out on the next autosave tick, not on
    /// every slider movement.
    config_dirty: bool,
    /// Fingerprint of the last session JSON written, so unchanged sessions
    /// aren't rewritten.
    last_session: u64,
    /// Held while this is the window that saves the session.
    session_lock: Option<crate::session::InstanceLock>,
    /// Whether this window saves the session and the bookmarks.
    session_owner: bool,
    /// Bumped by every message that may change what the session holds, so the
    /// autosave tick can skip the work when nothing happened.
    rev: u64,
    /// `rev` the session was last built at.
    session_rev: u64,
    /// How the current file stores its text (line endings, final newline).
    format: TextFormat,
    /// State on disk of the current file when it was last read or written,
    /// to notice edits made by other programs.
    disk_stamp: Option<DiskStamp>,
    /// Fingerprint of the text last read or saved, to see that an undo got
    /// back to it.
    saved_fp: Option<u64>,
    /// The tab the user last asked to switch to, so a slow read of another
    /// one doesn't take over.
    pending_open: Option<PathBuf>,
    /// Where to put the cursor (line, byte, length) once that file is the
    /// active tab: the definition being jumped to.
    pending_jump: Option<(PathBuf, usize, usize, usize)>,
    /// What the Preferences show about language servers.
    servers: crate::preferences::ServerStatus,
    /// Places left by jumps (definitions, go to line, bookmarks): Back and
    /// Forward walk through them.
    nav_back: Vec<Place>,
    nav_forward: Vec<Place>,
    /// Whether the "changed on disk" warning was already shown.
    external_warned: bool,
    /// Size on disk of `size_for`, cached so `view` does no I/O.
    file_size: Option<u64>,
    size_for: Option<PathBuf>,
    /// Whether the next edit should open a new undo group.
    /// True at start, after whitespace, after a cursor move, after delete.
    undo_new_group: bool,
    /// How the editor pane renders the buffer (Markdown / JSON files only).
    view_mode: ViewMode,
    /// Parsed preview of the buffer, rebuilt only when the text changes.
    preview: PreviewDoc,
    /// The status-bar label of the selection (`None` without one), kept up to
    /// date by `update` so `view` doesn't copy a possibly huge selection on
    /// every frame.
    selection_info: Option<String>,
    /// What `selection_info` was computed for (selection, text revision).
    selection_key: Option<(Option<crate::code_editor::SelectionRange>, u64)>,
    /// `text_rev` the preview was parsed at.
    preview_text_rev: u64,
    /// Bumped by every message that may change the buffer text.
    text_rev: u64,
    /// Folded node paths of the JSON tree.
    json_collapsed: std::sync::Arc<HashSet<String>>,
    /// Bumped whenever what the preview shows changes, so the cached widget
    /// tree (iced `lazy`) is rebuilt only then, not on every frame.
    preview_rev: u64,
    /// Closers typed by the auto-pairing and not stepped over yet:
    /// (line, byte column, character).
    pending_closers: Vec<(usize, usize, char)>,
    /// Ctrl/Cmd currently held, tracked for Ctrl+click on a chevron.
    mod_ctrl: bool,
    /// Alt currently held, tracked for Alt+click on a chevron.
    mod_alt: bool,
    mod_shift: bool,
}

/// Cap on the memory the undo history of one buffer may hold.
const UNDO_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Cap on the undo history of all tabs together.
const UNDO_TOTAL_BYTES: usize = 256 * 1024 * 1024;
const UNDO_MAX_STEPS: usize = 100;
/// Files larger than this are refused instead of freezing the editor.
const MAX_OPEN_BYTES: u64 = 64 * 1024 * 1024;
/// The same limit for pasted text.
/// A line longer than this makes layout and typing slow.
const LONG_LINE_BYTES: usize = 20_000;
const MAX_PASTE_BYTES: usize = 64 * 1024 * 1024;
/// Marker error for a dialog the user dismissed: not worth a status message.
const CANCELLED: &str = "cancelled";

/// What a background format run started from: the format result is only
/// applied if the buffer still reads the same.
#[derive(Debug, Clone)]
pub struct FormatJob {
    path: Option<PathBuf>,
    /// Fingerprint of the text that was sent (the whole buffer, or the
    /// selection).
    source: u64,
}

fn trim_history(stack: &mut History) {
    stack.trim(UNDO_MAX_STEPS, UNDO_MAX_BYTES);
}

fn history_bytes(stacks: &(History, History)) -> usize {
    stacks.0.bytes() + stacks.1.bytes()
}

/// The cached parse backing the preview pane.
enum PreviewDoc {
    None,
    Markdown(std::sync::Arc<Vec<crate::preview::markdown::Block>>),
    Json(std::sync::Arc<serde_json::Value>),
    JsonError(String),
}

impl SimpleEditApp {
    /// An app with one empty untitled tab, reading nothing from disk.
    fn blank(config: Config) -> Self {
        crate::lsp::set_completion_options(&config.completion_options);
        Self {
            bookmarks: crate::bookmarks::Bookmarks::default(),
            editor: EditorState::new(),
            file_marks: crate::projects::Marks::default(),
            finder: finder::FinderState::default(),
            projects: crate::projects::Projects::new(
                config.projects.clone(),
                config.projects_open.clone(),
                config.projects_hide_ignored,
                config.projects_exclude.clone(),
            ),
            lint: lint::LintState::default(),
            completion: completion::CompletionState::default(),
            split: None,
            sidebar: SidebarState::new(),
            search: SearchState::new(),
            preferences: PreferencesState::from_config(&config),
            show_search: false,
            show_sidebar: config.show_sidebar,
            // One panel at a time: with both asked for, the projects win once
            // there are some.
            mru: Vec::new(),
            tab_switch: None,
            closed_tabs: Vec::new(),
            preview_tab: None,
            preview_replaces: None,
            tree_click: None,
            tree_focus: false,
            tree_drag: None,
            tree_view: (0.0, 0.0),
            tab_list: None,
            panels: crate::sidebar::Panels {
                files: config.show_files_panel
                    && !(config.show_projects_panel && !config.projects.is_empty()),
                projects: config.show_projects_panel
                    && (!config.show_files_panel || !config.projects.is_empty()),
            },
            sidebar_width: config.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            sidebar_resizing: false,
            panel_height: PANEL_HEIGHT,
            panel_resizing: false,
            panel_drag: None,
            show_preferences: false,
            capturing: None,
            everywhere: everywhere::Everywhere::default(),
            results: results::Results::default(),
            open_menu: None,
            recents_open: false,
            menu_bounds: HashMap::new(),
            current_file: None,
            is_dirty: false,
            is_formatting: false,
            status_message: t!("status.ready").to_string(),
            terminal: Default::default(),
            scratch: crate::scratch::Panel::new(&config.scratch_dir),
            bookmark_panel: Default::default(),
            git: Default::default(),
            diff_tabs: HashMap::new(),
            diff_counter: 0,
            work_undo: Vec::new(),
            work_redo: Vec::new(),
            merge_tabs: HashMap::new(),
            merge_counter: 0,
            blame: None,
            marks: None,
            marks_asked: None,
            last_edit: std::time::Instant::now(),
            autosaving: false,
            ticks: 0,
            gutter_click: false,
            gutter_line: None,
            revert: None,
            conflict_toast_closed: false,
            server_offer: None,
            status_is_error: false,
            format_error: None,
            show_error_panel: false,
            untitled_counter: 1,
            file_cache: HashMap::new(),
            overlay: ActiveOverlay::None,
            goto_line_input: String::new(),
            undo_stack: History::default(),
            redo_stack: History::default(),
            undo_cache: HashMap::new(),
            config_dirty: false,
            last_session: 0,
            session_lock: None,
            session_owner: true,
            rev: 1,
            session_rev: 0,
            format: TextFormat::default(),
            disk_stamp: None,
            saved_fp: None,
            pending_open: None,
            pending_jump: None,
            servers: Default::default(),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            external_warned: false,
            file_size: None,
            size_for: None,
            undo_new_group: true,
            view_mode: ViewMode::Raw,
            preview: PreviewDoc::None,
            selection_info: None,
            selection_key: None,
            preview_text_rev: 0,
            text_rev: 1,
            json_collapsed: Default::default(),
            preview_rev: 0,
            pending_closers: Vec::new(),
            mod_ctrl: false,
            mod_alt: false,
            mod_shift: false,
            config,
        }
    }
}

impl SimpleEditApp {
    pub fn new(flags: Flags) -> (Self, Task<Message>) {
        // The first window owns the session; others start empty and leave it
        // alone, so closing them can't overwrite it.
        let lock = crate::session::InstanceLock::acquire();
        let owner = lock.is_some();
        let loaded = if owner {
            crate::session::Session::load()
        } else {
            crate::session::Loaded {
                session: crate::session::Session::default(),
                set_aside: None,
            }
        };
        let mut app = Self::blank(flags.config);
        app.git.pull_mode = app.config.git_pull_mode;
        app.git.merge_mode = app.config.git_merge_mode;
        app.session_lock = lock;
        app.session_owner = owner;
        if owner {
            app.bookmarks = crate::bookmarks::Bookmarks::load();
        } else {
            app.set_status(t!("status.second_window"), false);
        }

        app.bookmarks.prune_missing();
        let mut commands = vec![app.restore_session(loaded.session)];
        if let Some(aside) = loaded.set_aside {
            let name = aside
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            app.set_status(t!("status.session_set_aside", name = name.as_str()), true);
        }
        app.announce_new_version();

        // Files given on the command line open on top of the session.
        for path in flags.files {
            commands.push(Task::perform(read_file(path, true), Message::FileOpened));
        }
        (app, Task::batch(commands))
    }

    pub fn title(&self) -> String {
        let name = self
            .current_file
            .as_deref()
            .map(file_label)
            .unwrap_or_else(|| t!("app.untitled").to_string());
        let dirty = if self.is_dirty { " •" } else { "" };
        format!("SimpleEdit — {}{}", name, dirty)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // What the autosave and the preview have to look at again.
        let passive = message.is_background()
            || matches!(
                &message,
                Message::AutoSave
                    | Message::ModifiersChanged { .. }
                    | Message::MenuBounds(..)
                    | Message::ToggleMenu(_)
                    | Message::HoverMenu(_)
                    | Message::SetRecents(_)
                    | Message::SessionWritten(_)
                    | Message::PreviewTick
                    | Message::TerminalTick
                    | Message::WindowResized(..)
                    | Message::Git(crate::git_panel::GitMsg::Pick(_))
                    | Message::DiffTab(DiffNav::Pick(_) | DiffNav::Scrolled(..))
            );
        let edits_text = !passive && !matches!(&message, Message::EditorAction(a) if !a.is_edit());
        if edits_text {
            self.text_rev += 1;
        }
        if !passive {
            self.rev += 1;
        }
        // While typing in a preview, the parse waits for the next tick.
        let typing = matches!(&message, Message::EditorAction(a) if a.is_edit());
        if edits_text {
            self.last_edit = std::time::Instant::now();
        }
        let status_before = matches!(
            &message,
            Message::Git(crate::git_panel::GitMsg::StatusLoaded(_))
        )
        .then(|| self.git.status.clone());
        let git_done = matches!(&message, Message::Git(crate::git_panel::GitMsg::Done(_)));
        let command = self.handle(message);
        if edits_text {
            self.snippet_follow();
        }
        if self
            .completion
            .session
            .as_ref()
            .is_some_and(|s| self.current_file.as_ref() != Some(&s.path))
        {
            self.completion_close();
        }
        if self
            .completion
            .hint
            .as_ref()
            .is_some_and(|h| self.current_file.as_ref() != Some(h.path()))
        {
            self.completion.hint = None;
        }
        let command = if passive {
            command
        } else {
            Task::batch([command, self.completion_prepare()])
        };
        self.sync_split();
        let marks_task = self.after_message(edits_text, status_before, git_done);
        self.track_mru();
        self.settle_preview();
        let lint_task = self.lint_tick();
        let command = Task::batch([command, marks_task, lint_task]);
        if !typing {
            self.refresh_preview();
        }
        self.refresh_file_size();
        self.refresh_bookmark_panel();
        if !passive {
            self.refresh_selection_info();
        }
        command
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        // `listen_with` also sees events that a widget captured: the search
        // field swallows every key, the editor all arrows and Enter, and the
        // shortcuts must still work then.
        let keys = iced::event::listen_with(|event, status, _window| {
            use iced::event::Status;
            use iced::keyboard::{key::Named, Event, Key};

            let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
                return None;
            };
            if key == Key::Named(Named::Escape) {
                return Some(Message::EscapePressed);
            }

            let key_str = match key.as_ref() {
                Key::Character(c) => c.to_lowercase(),
                Key::Named(Named::Enter) => "Return".to_string(),
                Key::Named(Named::Tab) => "Tab".to_string(),
                Key::Named(Named::ArrowUp) => "ArrowUp".to_string(),
                Key::Named(Named::ArrowDown) => "ArrowDown".to_string(),
                Key::Named(Named::ArrowLeft) => "ArrowLeft".to_string(),
                Key::Named(Named::ArrowRight) => "ArrowRight".to_string(),
                Key::Named(Named::Home) => "Home".to_string(),
                Key::Named(Named::End) => "End".to_string(),
                Key::Named(Named::PageUp) => "PageUp".to_string(),
                Key::Named(Named::PageDown) => "PageDown".to_string(),
                Key::Named(Named::Delete) => "Delete".to_string(),
                Key::Named(Named::Insert) => "Insert".to_string(),
                Key::Named(Named::Backspace) => "Backspace".to_string(),
                Key::Named(Named::Space) => "Space".to_string(),
                Key::Named(Named::F1) => "F1".to_string(),
                Key::Named(Named::F2) => "F2".to_string(),
                Key::Named(Named::F3) => "F3".to_string(),
                Key::Named(Named::F4) => "F4".to_string(),
                Key::Named(Named::F5) => "F5".to_string(),
                Key::Named(Named::F6) => "F6".to_string(),
                Key::Named(Named::F7) => "F7".to_string(),
                Key::Named(Named::F8) => "F8".to_string(),
                Key::Named(Named::F9) => "F9".to_string(),
                Key::Named(Named::F10) => "F10".to_string(),
                Key::Named(Named::F11) => "F11".to_string(),
                Key::Named(Named::F12) => "F12".to_string(),
                _ => return None,
            };
            // Plain typing in a widget is none of the app's business (and
            // would wake it up on every keystroke); modified keys and keys a
            // widget left alone are.
            let ignored = status == Status::Ignored;
            let is_enter = key_str == "Return";
            if !(ignored || modifiers.command() || modifiers.alt() || is_enter) {
                return None;
            }

            Some(Message::KeyPressed {
                key: key_str,
                ctrl: modifiers.command(),
                shift: modifiers.shift(),
                alt: modifiers.alt(),
            })
        });

        // Shift twice: search everywhere.
        let double_shift = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(key)
                if everywhere::double_shift::hear(&key, std::time::Instant::now()) =>
            {
                Some(Message::DoubleShift)
            }
            _ => None,
        });

        // Chevron clicks need the modifier state at click time, which a button
        // press message does not carry.
        let modifiers = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged {
                    ctrl: m.command(),
                    alt: m.alt(),
                    shift: m.shift(),
                })
            }
            // The window is closed through the app so the session is saved.
            iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Quit),
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size.width, size.height))
            }
            // The back / forward buttons of a mouse.
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(Message::NavBack)
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Forward)) => {
                Some(Message::NavForward)
            }
            _ => None,
        });

        let autosave =
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::AutoSave);

        let mut subscriptions = vec![keys, double_shift, modifiers, autosave];
        if self.sidebar_resizing {
            subscriptions.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::SidebarResize(position.x))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::SidebarResizeEnd),
                    _ => None,
                },
            ));
        }
        if self.tree_drag.is_some() {
            subscriptions.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::TreeRelease),
                    _ => None,
                },
            ));
        }
        if self.panel_resizing {
            subscriptions.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::PanelResize(position.y))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::PanelResizeEnd),
                    _ => None,
                },
            ));
        }
        if self.view_mode != ViewMode::Raw && self.preview_kind().is_some() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(150))
                    .map(|_| Message::PreviewTick),
            );
        }
        if self.git.open {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_secs(2))
                    .map(|_| Message::Git(crate::git_panel::GitMsg::Tick)),
            );
        }
        if self.terminal.open {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(33))
                    .map(|_| Message::TerminalTick),
            );
        }
        iced::Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let dark = self.config.dark_mode;

        let title_bar = crate::menu_bar::view(&self.config, self.open_menu);

        let preview_kind = self.preview_kind();
        let view_mode = if preview_kind.is_some() {
            self.view_mode
        } else {
            ViewMode::Raw
        };
        let editor_with_context = self.editor_pane(preview_kind, view_mode, dark);
        let editor_with_context = self.with_split(editor_with_context, dark);

        // The search card floats over the top-right corner of the editor.
        let body: Element<'_, Message> = if self.show_search {
            crate::floating::top_right(
                editor_with_context,
                self.search.view(dark),
                12.0,
                // Clear of the minimap.
                16.0 + if self.config.show_minimap && view_mode != ViewMode::Preview {
                    crate::code_editor::MINIMAP_WIDTH
                } else {
                    0.0
                },
            )
        } else {
            editor_with_context
        };

        let (line, byte_col) = self.editor.content.cursor_position();
        // Columns count characters, not the bytes the editor indexes by.
        let cursor = (
            line,
            self.editor
                .content
                .line(line)
                .map_or(0, |l| l[..floor_boundary(&l, byte_col)].chars().count()),
        );
        let selection_info = self.selection_info.clone();
        let status_bar = crate::editor::statusbar::view(crate::editor::statusbar::Info {
            status: &self.status_message,
            is_error: self.status_is_error,
            offer: self.status_offer(),
            dark,
            cursor,
            line_ending: self.format.ending.label(),
            language: self.editor.language.as_deref(),
            selection: selection_info,
            file_size: self.file_size,
            use_spaces: self.config.use_spaces,
            tab_width: self.config.tab_width,
            branch: self
                .git
                .status
                .as_ref()
                .filter(|s| !s.branch.is_empty())
                .map(|s| (s.branch.clone(), s.ahead, s.behind)),
            problems: self.problem_counts(),
            problem_colors: (
                crate::lint::color(&self.config, crate::lint::Severity::Error),
                crate::lint::color(&self.config, crate::lint::Severity::Warning),
            ),
            lint_hint: self.lint_hint(),
        });

        let error_panel: Option<Element<Message>> = if self.show_error_panel {
            self.format_error.as_ref().map(|err| {
                let header = row![
                    text(t!("panel.errors").to_string())
                        .size(10.5)
                        .font(se_theme::UI_MEDIUM)
                        .color(se_theme::muted_text(dark)),
                    Space::new().width(Length::Fill),
                    button(text("×").size(16).color(se_theme::muted_text(dark)))
                        .padding([2, 6])
                        .on_press(Message::CloseErrorPanel)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style()
                        ),
                ]
                .padding([4, 14])
                .align_y(Alignment::Center);

                let body = scrollable(
                    container(
                        text(err.clone())
                            .size(12)
                            .color(se_theme::error_color(dark))
                            .font(se_theme::MONO),
                    )
                    .padding(iced::Padding {
                        top: 4.0,
                        right: 14.0,
                        bottom: 8.0,
                        left: 14.0,
                    })
                    .width(Length::Fill),
                )
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(100.0));

                container(column![rule(dark), header, body])
                    .width(Length::Fill)
                    .style(se_theme::error_panel(dark))
                    .into()
            })
        } else {
            None
        };

        // The editor panel: toolbar, (formatting banner), body, (errors), status.
        let mut panel = column![
            self.tab_bar(dark),
            self.toolbar_view(preview_kind, view_mode, dark),
            rule(dark)
        ];
        if self.is_formatting {
            panel = panel.push(
                container(
                    text(t!("status.formatting").to_string())
                        .size(12)
                        .color(se_theme::accent_color(dark)),
                )
                .width(Length::Fill)
                .padding([4, 14])
                .style(se_theme::accent_banner(dark)),
            );
        }
        panel = panel.push(body);
        if let Some(errors) = error_panel {
            panel = panel.push(errors);
        }
        if self.terminal.open {
            panel = panel.push(self.terminal_panel(dark));
        } else if self.git.open {
            panel =
                panel.push(container(self.git.view(dark)).height(Length::Fixed(self.panel_height)));
        }
        if self.scratch.open {
            panel = panel.push(self.scratch_panel(dark));
        }
        if self.bookmark_panel.open {
            panel = panel.push(self.bookmark_panel_view(dark));
        }
        if self.results.open && !self.results.tabs.is_empty() {
            panel = panel.push(self.results_view(dark));
        }
        let editor_panel = container(panel.push(status_bar))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(se_theme::editor_panel(dark));

        // The panels touch, split by hairlines; the edge between the list
        // and the editor is the handle that resizes the list.
        let mut main_row = row![].height(Length::Fill);
        main_row = main_row.push(self.activity_bar(dark)).push(vrule(dark));
        if self.show_sidebar && (self.panels.files || self.panels.projects) {
            main_row = main_row.push(
                container(self.sidebar.view(
                    dark,
                    &self.current_file,
                    &self.dirty_files(),
                    &self.bookmarks.marked_paths(),
                    &self.projects,
                    &self.file_marks,
                    self.panels,
                    self.sidebar_width,
                ))
                .width(Length::Fixed(self.sidebar_width)),
            );
            // The edge between the list and the editor (a strip of the list
            // and the hairline): drag it to resize.
            main_row = main_row.push(
                iced::widget::mouse_area(
                    row![
                        container(Space::new().width(SIDEBAR_GRIP).height(Length::Fill))
                            .style(se_theme::panel(dark)),
                        vrule(dark),
                    ]
                    .height(Length::Fill),
                )
                .on_press(Message::SidebarResizeStart)
                .interaction(iced::mouse::Interaction::ResizingHorizontally),
            );
        }
        main_row = main_row.push(editor_panel);

        let base: Element<Message> = container(column![title_bar, rule(dark), main_row])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(se_theme::window(dark))
            .into();

        // Floating dropdown (menu bar)
        // Bounds resolve one update cycle after a menu first opens (see
        // Message::ToggleMenu); until then, render nothing.
        let with_dropdown: Element<Message> = if let (Some(menu), Some(bounds)) = (
            self.open_menu,
            self.open_menu
                .and_then(|m| self.menu_bounds.get(&m).copied()),
        ) {
            let has_fmt = self
                .editor
                .language
                .as_deref()
                .map(crate::formatter::has_formatter)
                .unwrap_or(false);
            let lang_override_enabled = self.language_picker_enabled();
            let dropdown = crate::menu_bar::dropdown_view(
                menu,
                &self.config,
                has_fmt,
                lang_override_enabled,
                self.bookmark_state(),
                self.recents_open,
            );
            // Under the dropdown and below the menu bar (its titles stay
            // clickable), anywhere else closes the menu.
            let outside = column![
                Space::new().height(bounds.y + bounds.height),
                iced::widget::mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::CloseMenu)
                    .on_right_press(Message::CloseMenu),
            ];
            crate::floating::at_point(
                iced::widget::stack![base, outside],
                dropdown,
                bounds.x,
                bounds.y + bounds.height,
            )
        } else {
            base
        };

        // The modes behind the chevron of a Git button float where it was
        // pressed, over everything below; a click anywhere else closes them.
        let with_dropdown: Element<Message> = match self.git.split_overlay(dark) {
            Some((at, options)) => {
                let outside =
                    iced::widget::mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                        .on_press(Message::Git(crate::git_panel::GitMsg::CloseSplit))
                        .on_right_press(Message::Git(crate::git_panel::GitMsg::CloseSplit));
                // Right edge under the chevron, a little below the button.
                let x = (at.x - crate::git_panel::branches::SPLIT_OPTIONS_WIDTH + 14.0).max(4.0);
                crate::floating::at_point(
                    iced::widget::stack![with_dropdown, outside],
                    options,
                    x,
                    at.y + 14.0,
                )
            }
            None => with_dropdown,
        };

        // The card of a problem floats under it. The layer is always there,
        // empty when no card is open: the tree of widgets must keep its shape,
        // or the editor loses its focus (and its caret) each time a card
        // appears or goes away.
        let (card_at, card): (iced::Point, Element<Message>) = match self.lint_card(dark) {
            Some(open) => open,
            None => (iced::Point::ORIGIN, Space::new().into()),
        };
        let with_dropdown: Element<Message> =
            crate::floating::at_point(with_dropdown, card, card_at.x, card_at.y);
        // The list of completions, under the word being typed; its layer
        // is always there too, for the same reason.
        let (list_at, list): (iced::Point, Element<Message>) = match self.completion_view(dark) {
            Some(open) => open,
            None => (iced::Point::ORIGIN, Space::new().into()),
        };
        let with_dropdown: Element<Message> =
            crate::floating::at_point(with_dropdown, list, list_at.x, list_at.y);
        // The help on the parameters of the call being typed.
        let (hint_at, hint): (iced::Point, Element<Message>) = match self.hint_view(dark) {
            Some(open) => open,
            None => (iced::Point::ORIGIN, Space::new().into()),
        };
        let with_dropdown: Element<Message> =
            crate::floating::at_point(with_dropdown, hint, hint_at.x, hint_at.y);

        // The list of every tab, under its button; a click elsewhere closes it.
        let with_dropdown: Element<Message> = match self.tab_list {
            Some(at) => {
                let outside =
                    iced::widget::mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                        .on_press(Message::CloseTabList)
                        .on_right_press(Message::CloseTabList);
                crate::floating::at_point(
                    iced::widget::stack![with_dropdown, outside],
                    self.tab_list_view(dark),
                    (at.x - views::TAB_LIST_WIDTH + 16.0).max(4.0),
                    at.y + 14.0,
                )
            }
            None => with_dropdown,
        };

        // The Preferences window floats over everything but the dialogs.
        let with_prefs: Element<Message> = if self.show_preferences {
            let window = container(self.preferences.view(
                &self.config,
                self.capturing,
                &self.servers,
                self.current_file.as_deref(),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .max_width(900)
            .max_height(640);
            crate::floating::modal(
                with_dropdown,
                container(window).padding(40),
                Some(Message::TogglePreferences),
                dark,
                false,
            )
        } else {
            with_dropdown
        };

        // A pop-up at the top announces conflicts, until they are gone.
        let with_prefs: Element<Message> = match self.conflict_toast(dark) {
            Some(toast) => iced::widget::stack![with_prefs, toast].into(),
            None => with_prefs,
        };

        // Modal overlays
        match &self.overlay {
            ActiveOverlay::None => with_prefs,
            ActiveOverlay::Everywhere => {
                let card = self.view_everywhere(dark);
                crate::floating::modal(
                    with_prefs,
                    column![Space::new().height(70), card].align_x(Alignment::Center),
                    Some(Message::CloseOverlay),
                    dark,
                    true,
                )
            }
            ActiveOverlay::FindInFiles => {
                let card = self.view_find_in_files(dark);
                // Pinned, it stays when the editor is clicked.
                let blur = (!self.finder.pinned).then_some(Message::CloseOverlay);
                crate::floating::modal(
                    with_prefs,
                    column![Space::new().height(40), card].align_x(Alignment::Center),
                    blur,
                    dark,
                    true,
                )
            }
            ActiveOverlay::References(list) => {
                let card = self.view_references_overlay(dark, list);
                crate::floating::modal(
                    with_prefs,
                    column![Space::new().height(84), card].align_x(Alignment::Center),
                    Some(Message::CloseOverlay),
                    dark,
                    true,
                )
            }
            other => {
                let overlay = match other {
                    ActiveOverlay::About => self.view_about_overlay(dark),
                    ActiveOverlay::Changelog => {
                        self.changelog_card(dark, t!("changelog.title").to_string(), None)
                    }
                    ActiveOverlay::WhatsNew(previous) => self.changelog_card(
                        dark,
                        t!("changelog.whats_new", version = env!("CARGO_PKG_VERSION")).to_string(),
                        Some(previous.as_str()),
                    ),
                    ActiveOverlay::GotoLine => self.view_goto_line_overlay(dark),
                    ActiveOverlay::LanguagePicker => self.view_language_picker_overlay(dark),
                    ActiveOverlay::Shortcuts => self.view_shortcuts_overlay(dark),
                    ActiveOverlay::Conflicts => self.git.conflicts_card(dark),
                    ActiveOverlay::ConfirmOverwrite(path) => {
                        self.view_overwrite_overlay(dark, path)
                    }
                    ActiveOverlay::ConfirmClose(path) => self.view_confirm_overlay(dark, path),
                    ActiveOverlay::GitFile(path) => self.git_file_overlay(dark, path),
                    ActiveOverlay::GitPick(pick) => self.git_pick_overlay(dark, pick),
                    ActiveOverlay::BranchSwitch(filter) => self.branch_switch_overlay(dark, filter),
                    ActiveOverlay::ConfirmFileDelete(path, from_git) => {
                        // Tracked by git: a box to remove it from git too.
                        let extra: Option<Element<'static, Message>> = from_git.map(|on| {
                            iced::widget::checkbox(on)
                                .label(t!("delete_file.from_git").to_string())
                                .size(15)
                                .text_size(13)
                                .on_toggle(Message::ToggleDeleteFromGit)
                                .into()
                        });
                        confirm_card_with(
                            dark,
                            t!("delete_file.title").to_string(),
                            t!("delete_file.message", name = file_label(path).as_str()).to_string(),
                            extra,
                            t!("delete_file.confirm").to_string(),
                            Message::DeleteFile(path.clone(), from_git.unwrap_or(false)),
                        )
                    }
                    ActiveOverlay::ProjectName(dialog) => self.view_name_overlay(dark, dialog),
                    ActiveOverlay::ConfirmMove(source, dir) => confirm_card(
                        dark,
                        t!("projects.move_title").to_string(),
                        move_message(source, dir),
                        t!("projects.move").to_string(),
                        Message::MoveEntry(source.clone(), dir.clone()),
                    ),
                    ActiveOverlay::ConfirmFolderDelete(dir) => confirm_card(
                        dark,
                        t!("projects.delete_title").to_string(),
                        t!("projects.delete_message", name = file_label(dir).as_str()).to_string(),
                        t!("projects.delete_confirm").to_string(),
                        Message::DeleteFolder(dir.clone()),
                    ),
                    ActiveOverlay::ConfirmScratchDelete(path) => confirm_card(
                        dark,
                        t!("scratch.delete_title").to_string(),
                        t!("scratch.delete_message", name = file_label(path).as_str()).to_string(),
                        t!("scratch.delete").to_string(),
                        Message::ScratchDelete(path.clone()),
                    ),
                    ActiveOverlay::ConfirmCloseMany(paths, modified) => {
                        self.view_confirm_many_overlay(dark, paths, *modified)
                    }
                    ActiveOverlay::ConfirmQuit(error) => confirm_card(
                        dark,
                        t!("quit.title").to_string(),
                        t!("quit.message", error = error.as_str()).to_string(),
                        t!("quit.confirm").to_string(),
                        Message::QuitAnyway,
                    ),
                    _ => self.view_update_overlay(dark),
                };
                // A running download or install can't be dismissed: its
                // outcome would arrive without context. Escape reaches the
                // app through its own subscription, for every overlay.
                let on_blur = self
                    .overlay_is_dismissable()
                    .then_some(Message::CloseOverlay);
                crate::floating::modal(with_prefs, overlay, on_blur, dark, false)
            }
        }
    }

    pub fn theme(&self) -> Theme {
        if self.config.dark_mode {
            se_theme::ink_dark()
        } else {
            se_theme::ink_light()
        }
    }
}

// ─── Message handling ───────────────────────────────────────────────────────

impl Message {
    /// Answers of background work and timers: not an action of the user.
    fn is_background(&self) -> bool {
        match self {
            Message::Git(msg) => msg.is_background(),
            Message::BlameLoaded(..) | Message::MarksBase(..) => true,
            // What comes by itself (answers, timers) or from the pointer
            // merely passing over something: an open menu stays open.
            Message::FileStates(_)
            | Message::LintDone(..)
            | Message::LintFixesLoaded(..)
            | Message::LintCloseCheck(_)
            | Message::LintHover(_)
            | Message::LintCardHover(_)
            | Message::FinderFiles(_)
            | Message::FindResults(..)
            | Message::TreeScrolled(..)
            | Message::TreeHover(_)
            | Message::EverywhereFiles(..)
            | Message::Results(results::ResultsMsg::Rerun_(..))
            | Message::Results(results::ResultsMsg::Exported(_))
            | Message::EverywhereSymbolsDue(_)
            | Message::EverywhereSymbols(..)
            | Message::EverywhereTextDue(_)
            | Message::EverywhereText(..)
            | Message::EverywhereGit(..)
            | Message::CompletionInfo(..)
            | Message::CompletionAnswer(..)
            | Message::CompletionAnchor(..)
            | Message::HintAnswer(..)
            | Message::CompletionWheel(_)
            | Message::CompletionResolved(..)
            | Message::CompletionBusy(..) => true,
            _ => false,
        }
    }

    /// Shortcuts that act on the buffer and so shouldn't fire while a
    /// text field is being typed in.
    fn is_editor_shortcut(&self) -> bool {
        matches!(
            self,
            Message::Undo
                | Message::Redo
                | Message::SelectAll
                | Message::DuplicateLine
                | Message::MoveLineUp
                | Message::MoveLineDown
                | Message::ToggleComment
                | Message::DeleteLine
                | Message::InsertTab
                | Message::DedentLine
        )
    }
}

impl SimpleEditApp {
    fn handle(&mut self, message: Message) -> Task<Message> {
        // Passive/background messages must not close an open dropdown: X11
        // fires a spurious ModifiersChanged right after the very click that
        // opens the menu, AutoSave ticks every 5s regardless of the UI, and
        // MenuBounds is this same open answering its own position query.
        let closes_menu = !message.is_background()
            && !matches!(
                &message,
                Message::ToggleMenu(_)
                    | Message::HoverMenu(_)
                    | Message::SetRecents(_)
                    | Message::MenuBounds(..)
                    | Message::ModifiersChanged { .. }
                    | Message::AutoSave
                    | Message::SessionWritten(_)
                    | Message::PreviewTick
                    | Message::TerminalTick
                    | Message::WindowResized(..)
            );
        if closes_menu || matches!(message, Message::ToggleMenu(_) | Message::HoverMenu(_)) {
            self.recents_open = false;
        }
        if closes_menu {
            // Escape on an open menu closes only that menu.
            if matches!(message, Message::EscapePressed) && self.open_menu.take().is_some() {
                return Task::none();
            }
            self.open_menu = None;
        }

        if matches!(message, Message::Sidebar(_) | Message::NewFile) {
            self.git.typing = false;
        }

        match message {
            Message::EditorAction(action) => {
                self.git.typing = false;
                self.tree_focus = false;
                self.results.focus = false;
                // Back in the editor: Ctrl+A / Ctrl+C / Ctrl+Z are its own again.
                self.git.viewer.selection = None;
                self.git.stash.viewer.selection = None;
                if action.is_edit() {
                    self.blame = None;
                }
                // Without a tab there is nothing to edit (the next one is
                // still being read).
                if self.current_file.is_none() && action.is_edit() {
                    return Task::none();
                }
                // Lines added or removed by an edit move the bookmarks below.
                let probe = action.is_edit().then(|| self.edit_probe());
                let completion_action = action.clone();
                let command = self.editor_action(action);
                if let Some((count, top)) = probe {
                    self.shift_bookmarks(count, top);
                }
                let completion = self.completion_after_edit(&completion_action);
                Task::batch([command, completion])
            }
            Message::InsertTab => {
                if !self.editing_blocked() {
                    if self.selection_span().is_some() {
                        return self.handle(Message::IndentSelection);
                    }
                    // With a single-line selection this replaces it, as
                    // in most editors.
                    let text = self.tab_text();
                    self.push_undo();
                    self.undo_new_group = true;
                    self.edited(|app| {
                        app.editor.content.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(text)),
                        ));
                    });
                    self.is_dirty = true;
                }
                Task::none()
            }
            Message::SelectAll => {
                self.editor.content.perform(text_editor::Action::Move(
                    text_editor::Motion::DocumentStart,
                ));
                self.editor.content.perform(text_editor::Action::Select(
                    text_editor::Motion::DocumentEnd,
                ));
                Task::none()
            }
            Message::SetRecents(open) => {
                self.recents_open = open;
                Task::none()
            }
            Message::ClearRecents => {
                self.config.clear_recent();
                self.config.save();
                Task::none()
            }
            Message::ToggleMenu(menu) => {
                self.open_menu = if self.open_menu == Some(menu) {
                    None
                } else {
                    Some(menu)
                };
                self.query_menu_bounds()
            }
            Message::HoverMenu(menu) => {
                if self.open_menu.is_some() && self.open_menu != Some(menu) {
                    self.open_menu = Some(menu);
                    self.query_menu_bounds()
                } else {
                    Task::none()
                }
            }
            Message::MenuBounds(menu, bounds) => {
                if let Some(bounds) = bounds {
                    self.menu_bounds.insert(menu, bounds);
                }
                Task::none()
            }
            Message::NewFile => {
                self.stash_current();
                self.untitled_counter += 1;
                let path = untitled_path(self.untitled_counter);
                self.load_buffer(path, CachedBuffer::blank());
                self.set_status(t!("status.new_file"), false);
                Task::none()
            }
            Message::OpenFile => Task::perform(open_file(), Message::FileOpened),
            Message::FileOpened(result) => {
                match result {
                    Ok(loaded) => self.open_loaded(loaded),
                    Err(e) if e == CANCELLED => {}
                    Err(e) => self.set_status(format!("{}: {}", t!("status.error"), e), true),
                }
                Task::none()
            }
            Message::TabRead(result) => {
                match result {
                    // A slower read of a tab the user has left behind.
                    // (Its cached state stays for the next visit.)
                    Ok(loaded)
                        if self
                            .pending_open
                            .as_ref()
                            .is_some_and(|p| *p != loaded.path) => {}
                    Ok(loaded) => {
                        self.open_loaded(loaded);
                        self.apply_pending_jump();
                    }
                    Err(e) => {
                        self.pending_jump = None;
                        self.set_status(format!("{}: {}", t!("status.error"), e), true);
                        let failed = self.pending_open.take();
                        // Without a tab to show, drop the one that can't be read.
                        if self.current_file.is_none() {
                            if let Some(path) = failed {
                                self.sidebar.remove_path(&path);
                                self.file_cache.remove(&path);
                            }
                            return self.ensure_tab();
                        }
                    }
                }
                Task::none()
            }
            Message::FileReloaded(result) => {
                match result {
                    Ok(loaded) => self.apply_reload(loaded),
                    Err(e) => self.set_status(format!("{}: {}", t!("status.error"), e), true),
                }
                Task::none()
            }
            Message::SaveFile => {
                self.fix_before_save();
                if let Some(path) = self.current_file.clone() {
                    if self.changed_on_disk(&path) {
                        self.overlay = ActiveOverlay::ConfirmOverwrite(path);
                        return Task::none();
                    }
                }
                let content = crate::editor::buffer_text(&self.editor.content);
                let origin = self.current_file.clone();
                match origin.clone() {
                    Some(path) if !is_untitled(&path) => {
                        save_command(origin, save_file(path, content, self.format))
                    }
                    Some(_) => self.start_save_as(origin),
                    None => Task::none(),
                }
            }
            Message::SaveFileAs => self.start_save_as(self.current_file.clone()),
            Message::SaveAsChosen(origin, result) => match result {
                Ok(path) => {
                    // Another tab holds unsaved work for that file: writing
                    // over the file would silently drop it.
                    let elsewhere = origin.as_ref() != Some(&path)
                        && self.sidebar.files().contains(&path)
                        && self.is_path_dirty(&path);
                    if elsewhere {
                        self.set_status(
                            t!("status.save_as_open_tab", name = file_label(&path).as_str()),
                            true,
                        );
                        return Task::none();
                    }
                    // Whatever the tab holds now is what gets written.
                    let content = if origin == self.current_file {
                        crate::editor::buffer_text(&self.editor.content)
                    } else {
                        origin
                            .as_ref()
                            .and_then(|o| self.file_cache.get(o))
                            .and_then(CachedBuffer::text)
                            .unwrap_or_default()
                    };
                    let format = if origin == self.current_file {
                        self.format
                    } else {
                        origin
                            .as_ref()
                            .and_then(|o| self.file_cache.get(o))
                            .map(|c| c.format)
                            .unwrap_or_default()
                    };
                    save_command(origin, save_file(path, content, format))
                }
                Err(e) if e == CANCELLED => Task::none(),
                Err(e) => {
                    self.set_status(format!("{}: {}", t!("status.error"), e), true);
                    Task::none()
                }
            },
            Message::FileSaved(origin, result) => {
                match result {
                    Ok((path, saved)) => {
                        self.size_for = None;
                        // The tab the save started from may no longer be the
                        // active one (the dialog is asynchronous).
                        let from_current = origin == self.current_file;
                        if let Some(old) = origin.as_ref().filter(|o| **o != path) {
                            // "Save as" turns the tab into the new file; the
                            // old file stays as it is on disk.
                            if self.sidebar.files().contains(&path) {
                                self.sidebar.remove_path(&path);
                                self.undo_cache.remove(&path);
                            }
                            self.file_cache.remove(&path);
                            self.sidebar.rename_file(old, path.clone());
                            self.bookmarks.rename(old, &path);
                            if !from_current {
                                if let Some(entry) = self.file_cache.remove(old) {
                                    self.file_cache.insert(path.clone(), entry);
                                }
                                if let Some(stacks) = self.undo_cache.remove(old) {
                                    self.undo_cache.insert(path.clone(), stacks);
                                }
                            }
                        }
                        if !self.is_scratch(&path) {
                            self.sidebar.add_file(path.clone());
                        }
                        let saved_fp = fingerprint(&saved);
                        let stamp = DiskStamp::of(&path);
                        if from_current {
                            let first_line = first_line_of(&self.editor.content);
                            self.editor.adopt_path_language(&path, &first_line);
                            // Edits made while the save was in flight stay dirty.
                            let now = crate::editor::buffer_text(&self.editor.content);
                            self.is_dirty = now != saved;
                            self.saved_fp = Some(saved_fp);
                            self.disk_stamp = stamp;
                            self.external_warned = false;
                            self.current_file = Some(path.clone());
                        } else if let Some(entry) = self.file_cache.get_mut(&path) {
                            entry.dirty = entry.text().is_some_and(|t| t != saved);
                            entry.saved_fp = Some(saved_fp);
                            entry.stamp = stamp;
                        }
                        if !self.is_scratch(&path) {
                            self.config.add_recent(&path);
                            self.config_dirty = true;
                        }
                        if !self.autosaving {
                            self.set_status(t!("status.file_saved"), false);
                        }
                        self.autosaving = false;
                        // The tools that read the disk look again.
                        self.lint.force = self.config.lint_trigger != crate::lint::Trigger::Manual;
                        // The Git panel and the open diffs follow the save.
                        let panel = if self.git.open {
                            Task::done(Message::Git(crate::git_panel::GitMsg::Refresh))
                        } else {
                            Task::none()
                        };
                        return Task::batch([panel, self.refresh_diff_tabs()]);
                    }
                    Err(e) if e == CANCELLED => self.autosaving = false,
                    Err(e) => {
                        self.autosaving = false;
                        self.set_status(format!("{}: {}", t!("status.error"), e), true);
                    }
                }
                Task::none()
            }
            Message::CloseOtherTabs => self.handle(Message::CloseTabs(self.current_file.clone())),
            Message::CloseTabs(keep) => self.request_close_many(keep),
            Message::ConfirmDiscardMany(paths) => {
                self.overlay = ActiveOverlay::None;
                self.close_many(paths)
            }
            Message::CloseFile => match self.current_file.clone() {
                // Scratch files stay until they are deleted.
                Some(path) if self.is_scratch(&path) => Task::none(),
                Some(path) => self.request_close(path),
                None => Task::none(),
            },
            // The menu closed above, like for any message that isn't passive.
            Message::CloseMenu => Task::none(),
            Message::MarksBase(path, base) => {
                self.marks_base_loaded(path, base);
                Task::none()
            }
            Message::BlameFile => self.start_blame(),
            Message::BlameLoaded(path, result) => {
                self.blame_loaded(path, result);
                Task::none()
            }
            Message::CloseBlame => {
                self.blame = None;
                Task::none()
            }
            Message::BlameLineClicked(line) => self.blame_line_clicked(line),
            Message::BlamePrevious => {
                let (Some(line), Some(blame)) = (self.gutter_line, &self.blame) else {
                    return Task::none();
                };
                let Some(sha) = blame
                    .shas
                    .get(line)
                    .filter(|s| !s.chars().all(|c| c == '0'))
                else {
                    return Task::none();
                };
                let (repo, relative) = (blame.repo.clone(), blame.relative.clone());
                let sha = sha.clone();
                Task::perform(
                    async move {
                        let rev = format!("{sha}^");
                        let lines =
                            crate::git::history::blame_at(&repo, Some(&rev), &relative).await?;
                        let text = crate::git::history::file_at(&repo, &rev, &relative).await?;
                        let label = sha.chars().take(8).collect::<String>();
                        Ok((repo, relative, label, text, lines))
                    },
                    Message::BlamePreviousLoaded,
                )
            }
            Message::BlamePreviousLoaded(result) => {
                match result {
                    Ok((repo, relative, label, text, lines)) => {
                        // A tab of its own for that version, with its blame.
                        self.untitled_counter += 1;
                        let name = relative.rsplit('/').next().unwrap_or(&relative).to_string();
                        let path = PathBuf::from(format!(
                            "rev://{}/{name} @ {label}",
                            self.untitled_counter
                        ));
                        let language = Path::new(&relative)
                            .extension()
                            .and_then(|e| e.to_str())
                            .and_then(|e| crate::editor::extension_to_token(&e.to_lowercase()))
                            .map(str::to_string);
                        self.stash_current();
                        self.sidebar.add_file(path.clone());
                        let loaded = Loaded {
                            path: path.clone(),
                            text,
                            format: TextFormat::default(),
                            stamp: None,
                        };
                        self.load_buffer(path.clone(), CachedBuffer::from_loaded(loaded, language));
                        self.blame_loaded(path, Ok((repo, relative, lines)));
                    }
                    Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
                }
                Task::none()
            }
            Message::OpenConflicts => {
                if self
                    .git
                    .status
                    .as_ref()
                    .is_some_and(|s| s.conflicts().next().is_some())
                {
                    self.overlay = ActiveOverlay::Conflicts;
                }
                Task::none()
            }
            Message::OpenMergeTab(spec) => self.open_merge_tab(spec),
            Message::Merge(msg) => self.merge_msg(msg),
            Message::MergeApplied(path, result) => match result {
                Ok(()) => {
                    let close = self.close_path(path);
                    Task::batch([
                        close,
                        Task::done(Message::Git(crate::git_panel::GitMsg::Refresh)),
                    ])
                }
                Err(error) => {
                    self.set_status(format!("{}: {error}", t!("status.error")), true);
                    Task::none()
                }
            },
            Message::DiffApplied(result) => {
                if let Err(error) = result {
                    self.set_status(format!("{}: {error}", t!("status.error")), true);
                }
                Task::batch([
                    self.refresh_diff_tabs(),
                    Task::done(Message::Git(crate::git_panel::GitMsg::Refresh)),
                ])
            }
            Message::OpenDiffTab(tab) => self.open_diff_tab(tab),
            Message::DiffTab(nav) => self.diff_nav(nav),
            Message::ToggleGit => {
                let ctx = crate::git_panel::Ctx {
                    current_file: self.current_file.as_deref(),
                    ctrl: self.mod_ctrl,
                    shift: self.mod_shift,
                };
                let task = self.git.toggle(&ctx);
                if self.git.open {
                    // One tool panel at the bottom: the terminal steps aside.
                    self.terminal.open = false;
                    self.terminal.focused = false;
                    self.scratch.open = false;
                    self.bookmark_panel.open = false;
                }
                task
            }
            Message::Git(msg) => {
                // The panel closed: what went wrong is told in the status bar.
                if let crate::git_panel::GitMsg::Done(Err(error)) = &msg {
                    if !self.git.open && error != crate::git::CONFLICT {
                        self.set_status(error.clone(), true);
                    }
                }
                self.remember_before_git(&msg);
                let ctx = crate::git_panel::Ctx {
                    current_file: self.current_file.as_deref(),
                    ctrl: self.mod_ctrl,
                    shift: self.mod_shift,
                };
                let refreshing = matches!(
                    msg,
                    crate::git_panel::GitMsg::Tick | crate::git_panel::GitMsg::Refresh
                );
                // A diff may be the answer a tab in the editor waits for.
                let tabs = if let crate::git_panel::GitMsg::DiffLoaded(token, result) = &msg {
                    let tasks: Vec<_> = self
                        .diff_tabs
                        .values_mut()
                        .filter(|tab| tab.viewer.owns(*token))
                        .map(|tab| tab.viewer.loaded(*token, result.clone()))
                        .collect();
                    Task::batch(tasks)
                } else {
                    Task::none()
                };
                let panel = self.git.update(msg, &ctx);
                let again = if refreshing {
                    self.refresh_diff_tabs()
                } else {
                    Task::none()
                };
                Task::batch([panel, tabs, again])
            }
            Message::ToggleTerminal => {
                if self.terminal.open {
                    self.terminal.open = false;
                    self.terminal.focused = false;
                } else {
                    if self.terminal.tabs.is_empty() {
                        self.new_terminal();
                    }
                    self.terminal.open = !self.terminal.tabs.is_empty();
                    self.terminal.focused = self.terminal.open;
                    if self.terminal.open {
                        // One tool panel at the bottom.
                        self.scratch.open = false;
                        self.bookmark_panel.open = false;
                        self.git.open = false;
                    }
                }
                Task::none()
            }
            Message::ToggleScratch => {
                self.scratch.open = !self.scratch.open;
                if self.scratch.open {
                    self.terminal.open = false;
                    self.terminal.focused = false;
                    self.bookmark_panel.open = false;
                    self.git.open = false;
                    self.scratch.refresh();
                    self.scratch.error = None;
                }
                Task::none()
            }
            Message::GitFileMenu(path) => {
                self.overlay = ActiveOverlay::GitFile(path);
                Task::none()
            }
            Message::GitFileDo(path, action) => self.git_file_do(path, action),
            Message::GitFileRoot(path, action, root) => self.git_file_root(path, action, root),
            Message::GitFileDone(result) => self.git_file_done(result),
            Message::GitFileDiff(result) => self.git_file_diff(result),
            Message::GitFilePick(result) => self.git_file_pick(result),
            Message::GitPickFilter(filter) => {
                if let ActiveOverlay::GitPick(pick) = &mut self.overlay {
                    pick.filter = filter;
                }
                Task::none()
            }
            Message::GitPickChoose(rev) => self.git_pick_choose(rev),
            Message::GitPickVerified(result) => self.git_pick_verified(result),
            Message::ToggleBookmarkPanel => {
                self.bookmark_panel.open = !self.bookmark_panel.open;
                if self.bookmark_panel.open {
                    self.terminal.open = false;
                    self.terminal.focused = false;
                    self.scratch.open = false;
                    self.git.open = false;
                    self.refresh_bookmark_texts(true);
                }
                Task::none()
            }
            Message::BookmarkClick(target) => self.bookmark_click(target),
            Message::BookmarkFold(path) => {
                if !self.bookmark_panel.collapsed.remove(&path) {
                    self.bookmark_panel.collapsed.insert(path);
                }
                Task::none()
            }
            Message::BookmarkDelete(target) => {
                self.delete_bookmark(target);
                Task::none()
            }
            Message::ToggleFileBookmark(path) => {
                self.toggle_file_bookmark(path);
                Task::none()
            }
            Message::ScratchName(name) => {
                self.scratch.name = name;
                self.scratch.error = None;
                Task::none()
            }
            Message::ScratchExt(ext) => {
                self.scratch.ext = ext;
                self.scratch.error = None;
                Task::none()
            }
            Message::ScratchCreate => {
                let ext = self.scratch.ext.as_str();
                let name = if self.scratch.name.trim().is_empty() {
                    crate::scratch::next_name(&self.scratch.dir, ext)
                } else {
                    self.scratch.name.clone()
                };
                match crate::scratch::create(&self.scratch.dir, &name, ext) {
                    Ok(path) => {
                        self.scratch.name.clear();
                        self.scratch.error = None;
                        self.scratch.refresh();
                        self.open_scratch(path)
                    }
                    Err(e) => {
                        self.scratch.error = Some(e);
                        Task::none()
                    }
                }
            }
            Message::ScratchOpen(path) => self.open_scratch(path),
            Message::ScratchAskDelete(path) => {
                self.overlay = ActiveOverlay::ConfirmScratchDelete(path);
                Task::none()
            }
            Message::ScratchDelete(path) => {
                self.overlay = ActiveOverlay::None;
                let command = self.close_path(path.clone());
                self.bookmarks.clear(&path);
                if let Err(e) = std::fs::remove_file(&path) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        self.scratch.error = Some(e.to_string());
                    }
                }
                self.scratch.refresh();
                command
            }
            Message::PickScratchDir => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title(t!("scratch.folder_title").to_string())
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::ScratchDirChosen,
            ),
            Message::ScratchDirChosen(Some(dir)) => {
                self.set_scratch_dir(dir.to_string_lossy().into_owned())
            }
            Message::ScratchDirChosen(None) => Task::none(),
            Message::SetScratchDir(dir) => self.set_scratch_dir(dir),
            Message::NewTerminal => {
                self.new_terminal();
                self.terminal.open = !self.terminal.tabs.is_empty();
                self.terminal.focused = self.terminal.open;
                Task::none()
            }
            Message::SelectTerminal(index) => {
                if index < self.terminal.tabs.len() {
                    self.terminal.active = index;
                    self.terminal.focused = true;
                }
                Task::none()
            }
            Message::CloseTerminal(index) => {
                self.terminal.close(index);
                Task::none()
            }
            Message::TerminalTick => {
                self.terminal.prune_ended();
                Task::none()
            }
            Message::TerminalFocus(focused) => {
                self.terminal.focused = focused && self.terminal.open;
                Task::none()
            }
            Message::TerminalCopy(text) => iced::clipboard::write(text),
            Message::TerminalPaste => iced::clipboard::read().map(Message::TerminalPasteText),
            Message::TerminalPasteText(text) => {
                if let (Some(text), Some(session)) = (text, self.terminal.active_session()) {
                    let bracketed = session
                        .with_screen(|screen| screen.bracketed_paste())
                        .unwrap_or(false);
                    if bracketed {
                        session.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                    } else {
                        // A pasted line break is the Enter key.
                        session.write(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                    }
                }
                Task::none()
            }
            Message::ConfirmOverwrite => {
                self.overlay = ActiveOverlay::None;
                // Take the new on-disk state as the baseline, then save.
                self.disk_stamp = self.current_file.as_deref().and_then(DiskStamp::of);
                self.handle(Message::SaveFile)
            }
            Message::ConfirmDiscard(path) => {
                self.overlay = ActiveOverlay::None;
                self.close_path(path)
            }
            Message::ToggleSearch => {
                self.show_search = !self.show_search;
                if self.show_search {
                    // Ready to type, as the card opens.
                    iced::widget::operation::focus(crate::search::find_field_id())
                } else {
                    Task::none()
                }
            }
            Message::Lint(setting) => {
                // Turning a language on looks at the file at once.
                if matches!(setting, crate::lint::Setting::Language(_, true)) {
                    self.lint.force = true;
                }
                crate::lint::apply_setting(&mut self.config, setting);
                self.config_dirty = true;
                Task::none()
            }
            Message::LintDone(generation, key, outcome) => {
                self.lint_done(generation, key, outcome);
                Task::none()
            }
            Message::LintHover(over) => self.lint_hover(over),
            Message::CompletionInvoke => {
                self.completion_request(crate::lsp::Trigger::Invoked, true)
            }
            Message::CompletionInfo(key, info) => {
                self.completion_info_loaded(key, info);
                Task::none()
            }
            Message::CompletionAnswer(generation, answer) => {
                let (base, found) = *answer;
                self.completion_answer(generation, base, found)
            }
            Message::CompletionAnchor(list, hint) => {
                if let Some(session) = self.completion.session.as_mut() {
                    session.anchor_at = list;
                }
                if let Some(open) = self.completion.hint.as_mut() {
                    open.anchor_at = hint;
                }
                Task::none()
            }
            Message::HintInvoke => self.hint_request(1, None, true),
            Message::HintAnswer(generation, answer) => self.hint_answer(generation, answer),
            Message::CompletionPick(pos) => self.completion_accept(pos, false),
            Message::CompletionWheel(lines) => {
                self.completion_wheel(lines);
                Task::none()
            }
            Message::CompletionResolved(of, index, resolved) => {
                self.completion_resolved(of, index, resolved);
                Task::none()
            }
            Message::CompletionImport(token, resolved) => self.completion_import(token, resolved),
            Message::CompletionBusy(generation, busy) => {
                self.completion_busy_answer(generation, busy)
            }
            Message::LintCardHover(over) => self.lint_card_hovered(over),
            Message::LintCloseCheck(token) => {
                self.lint_close_check(token);
                Task::none()
            }
            Message::LintFixesLoaded(key, id, result) => {
                self.lint_fixes_loaded(key, id, result);
                Task::none()
            }
            Message::LintApply(id, fix) => self.lint_apply(id, fix),
            Message::LintFixAll => {
                self.lint.hover = None;
                self.start_fix_all()
            }
            Message::LintFixSource(source) => self.start_fix_source(source),
            Message::LintFixedAll(key, text) => self.lint_fixed_all(key, text),
            Message::LintFixFile(path) => self.lint_fix_file(path),
            Message::LintNow => self.lint_now(),
            Message::AddProject => Task::perform(pick_folder(), Message::ProjectPicked),
            Message::ProjectPicked(Some(dir)) => {
                if self.projects.add(dir) {
                    self.save_projects();
                }
                Task::none()
            }
            Message::ProjectPicked(None) => Task::none(),
            Message::ToggleProjectFolder(dir) => {
                self.tree_focus = true;
                self.projects.selected = Some(dir.clone());
                self.projects.toggle(&dir);
                self.save_projects();
                Task::none()
            }
            Message::RemoveProject(root) => {
                self.projects.remove(&root);
                self.save_projects();
                Task::none()
            }
            Message::NewProjectEntry(dir, folder) => self.ask_name(if folder {
                project_ops::NameFor::Folder(dir)
            } else {
                project_ops::NameFor::File(dir)
            }),
            Message::RenameProjectEntry(path) => self.ask_name(project_ops::NameFor::Rename(path)),
            Message::ProjectNameChanged(text) => {
                self.name_typed(text);
                Task::none()
            }
            Message::ProjectNameSubmit => self.name_submitted(),
            Message::AskDeleteFolder(dir) => {
                self.ask_delete_folder(dir);
                Task::none()
            }
            Message::DeleteFolder(dir) => self.delete_folder(dir),
            Message::FolderDeleted(result) => self.folder_deleted(result),
            Message::CopyPath(path) => iced::clipboard::write(path.to_string_lossy().into_owned()),
            Message::ToggleHideIgnored => {
                let hide = !self.projects.hide_ignored;
                self.projects.set_hide_ignored(hide);
                self.config.projects_hide_ignored = hide;
                self.config_dirty = true;
                Task::none()
            }
            Message::TreePress(path) => {
                self.tree_focus = true;
                self.projects.selected = Some(path.clone());
                self.tree_drag = Some((path, false));
                Task::none()
            }
            Message::TreeHover(over) => {
                if let Some((source, moved)) = &mut self.tree_drag {
                    if *source != over {
                        *moved = true;
                    }
                    if *moved {
                        // Onto a file: into its folder.
                        let dir = if over.is_dir() {
                            Some(over)
                        } else {
                            over.parent().map(Path::to_path_buf)
                        };
                        let source = source.clone();
                        self.projects.drop_target = dir
                            .filter(|d| movable(&source, d))
                            .filter(|d| self.projects.roots.iter().any(|r| d.starts_with(r)));
                    }
                }
                Task::none()
            }
            Message::TreeRelease => {
                let drag = self.tree_drag.take();
                let target = self.projects.drop_target.take();
                if let (Some((source, true)), Some(dir)) = (drag, target) {
                    self.overlay = ActiveOverlay::ConfirmMove(source, dir);
                }
                Task::none()
            }
            Message::MoveEntry(source, dir) => {
                self.overlay = ActiveOverlay::None;
                let Some(name) = source.file_name() else {
                    return Task::none();
                };
                let target = dir.join(name);
                if target.exists() {
                    let name = name.to_string_lossy().into_owned();
                    self.set_status(t!("projects.exists", name = name.as_str()), true);
                    return Task::none();
                }
                self.projects.open(&dir);
                self.move_entry(source, target)
            }
            Message::TreeScrolled(offset, height) => {
                self.tree_view = (offset, height);
                Task::none()
            }
            Message::CollapseProjects => {
                self.projects.collapse_all();
                self.save_projects();
                Task::none()
            }
            Message::SetProjectsExclude(typed) => {
                let patterns: Vec<String> = typed
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                self.preferences.exclude_draft = patterns.join(", ");
                self.projects.set_exclude(patterns.clone());
                self.config.projects_exclude = patterns;
                self.config_dirty = true;
                Task::none()
            }
            Message::OpenTerminalAt(dir) => {
                if let Err(error) = self.terminal.add(&dir, &t!("terminal.local")) {
                    self.set_status(format!("{}: {error}", t!("status.error")), true);
                    return Task::none();
                }
                self.terminal.open = true;
                self.terminal.focused = true;
                self.scratch.open = false;
                self.bookmark_panel.open = false;
                self.git.open = false;
                Task::none()
            }
            Message::RevealInFileManager(path) => {
                if let Err(e) = reveal_in_file_manager(&path) {
                    self.set_status(format!("{}: {e}", t!("status.error")), true);
                }
                Task::none()
            }
            Message::DuplicateEntry(path) => self.ask_name(project_ops::NameFor::Duplicate(path)),
            Message::ShowInProjects(file) => {
                if !self.projects.show(&file) {
                    return Task::none();
                }
                self.save_projects();
                // The tree is brought up: the sidebar, with the projects.
                self.panels = crate::sidebar::Panels {
                    files: false,
                    projects: true,
                };
                self.show_sidebar = true;
                self.config.show_sidebar = true;
                self.config.show_files_panel = false;
                self.config.show_projects_panel = true;
                self.config_dirty = true;
                // And scrolled to the file.
                let index = self
                    .projects
                    .rows()
                    .iter()
                    .position(|r| r.entry.path == file)
                    .unwrap_or(0);
                iced::widget::operation::scroll_to(
                    crate::sidebar::tree_id(),
                    iced::widget::scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: crate::sidebar::tree_offset(index),
                    },
                )
            }
            Message::FileStates(states) => {
                self.file_marks.set_git(states);
                Task::none()
            }
            Message::RefreshProjects => {
                self.projects.refresh(true);
                Task::none()
            }
            Message::OpenLintSettings => {
                self.show_preferences = true;
                self.handle(Message::Preferences(PreferencesMessage::PageSelected(
                    crate::preferences::PrefsPage::Lint,
                )))
            }
            Message::LintNext(forward) => {
                self.lint_next(forward);
                Task::none()
            }
            Message::Split(side_by_side) => self.split_editor(side_by_side, None),
            Message::SplitWith(path, side_by_side) => self.split_editor(side_by_side, Some(path)),
            Message::CloseSplit => match self.split.as_ref().map(|s| s.active) {
                Some(active) => self.close_pane(active),
                None => Task::none(),
            },
            Message::ClosePane(id) => self.close_pane(id),
            Message::SplitFocus(id) => self.focus_pane(id),
            Message::SplitAction(id, action) => self.split_action(id, action),
            Message::TabMoved(from, to) => {
                self.sidebar.move_tab(from, to);
                Task::none()
            }
            Message::TreeOpenFile(path) => self.tree_open(path),
            Message::KeepTab => {
                self.preview_tab = None;
                Task::none()
            }
            Message::TabDoubleClick(index) => {
                if self.preview_tab.is_some()
                    && self.preview_tab.as_ref() == self.sidebar.files().get(index)
                {
                    self.preview_tab = None;
                }
                Task::none()
            }
            Message::CloseTabAt(index) => match self.sidebar.files().get(index).cloned() {
                Some(path) => self.request_close(path),
                None => Task::none(),
            },
            Message::ReopenTab => self.reopen_tab(),
            Message::OpenGotoFile => self.open_everywhere(crate::everywhere::Tab::Files),
            Message::OpenFindInFiles => self.open_find_in_files(),
            Message::FinderFiles(files) => self.finder_files(files),
            Message::FindQuery(text) => {
                self.finder.text = text;
                self.search_files(true)
            }
            Message::FindToggleCase => {
                self.finder.case_sensitive = !self.finder.case_sensitive;
                self.search_files(false)
            }
            Message::FindToggleRegex => {
                self.finder.regex = !self.finder.regex;
                self.search_files(false)
            }
            Message::FindResults(generation, result) => {
                self.find_results(generation, result);
                Task::none()
            }
            Message::FindOpen(pick) => self.find_open(pick),
            Message::FindInFolder(dir) => self.find_in_folder(dir, false),
            Message::ReplaceInFolder(dir) => self.find_in_folder(dir, true),
            Message::OpenReplaceInFiles => self.open_finder(true),
            Message::FindReplaceText(text) => {
                self.finder.replace = text;
                self.search_files(true)
            }
            Message::FindToggleMultiline => self.finder_toggle_multiline(),
            Message::FindIgnored(on) => {
                self.config.find_ignored = on;
                self.config_dirty = true;
                self.load_scope_files()
            }
            Message::FindLines(action) => self.finder_lines_action(action),
            Message::FindToggleWords => {
                self.finder.words = !self.finder.words;
                self.search_files(false)
            }
            Message::FindTogglePreserve => {
                self.finder.preserve_case = !self.finder.preserve_case;
                self.search_files(false)
            }
            Message::FindMode(replace) => {
                self.finder.replace_mode = replace;
                self.search_files(false)
            }
            Message::FindContext(context) => {
                self.finder.context = context;
                self.finder.menu = None;
                self.search_files(false)
            }
            Message::FindMaskOn(on) => {
                self.finder.mask_on = on;
                self.search_files(false)
            }
            Message::FindMask(mask) => {
                self.finder.mask = mask;
                self.search_files(true)
            }
            Message::FindScope(scope) => {
                let reload = (scope == finder::Scope::Directory)
                    != (self.finder.scope == finder::Scope::Directory);
                self.finder.scope = scope;
                if reload {
                    self.load_scope_files()
                } else {
                    self.search_files(false)
                }
            }
            Message::FindRecursive => {
                self.finder.recursive = !self.finder.recursive;
                self.search_files(false)
            }
            Message::FindMenu(menu) => {
                self.finder.menu = if self.finder.menu == menu { None } else { menu };
                Task::none()
            }
            Message::FindHistory(past) => {
                self.finder.multiline = past.contains('\n');
                self.finder_set_text(past);
                self.finder.menu = None;
                self.search_files(false)
            }
            Message::FindPin => {
                self.finder.pinned = !self.finder.pinned;
                Task::none()
            }
            Message::FindSelect(index) => {
                self.finder_select(index);
                Task::none()
            }
            Message::FindReplaceOne => self.finder_replace_one(),
            Message::FindReplaceAll => {
                self.finder.confirm_all = true;
                Task::none()
            }
            Message::FindReplaceConfirm(yes) => {
                if yes {
                    self.finder_replace_all()
                } else {
                    self.finder.confirm_all = false;
                    Task::none()
                }
            }
            Message::FindUndoReplace => self.undo_replacement(),
            Message::FindToWindow => self.finder_to_window(),
            Message::Results(msg) => self.results_update(msg),
            Message::OpenTabList(at) => {
                self.tab_list = Some(at);
                Task::none()
            }
            Message::CloseTabList => {
                self.tab_list = None;
                Task::none()
            }
            Message::PickTab(path) => {
                self.tab_list = None;
                self.activate(path)
            }
            Message::TogglePanel(panel) => {
                // One panel at a time: the one asked for takes the place of
                // the other, and asking for the one that is open closes it.
                let was_open = match panel {
                    crate::sidebar::Panel::Files => self.panels.files,
                    crate::sidebar::Panel::Projects => self.panels.projects,
                };
                self.panels = crate::sidebar::Panels {
                    files: panel == crate::sidebar::Panel::Files && !was_open,
                    projects: panel == crate::sidebar::Panel::Projects && !was_open,
                };
                // A panel that is asked for shows the sidebar if it was hidden.
                if !was_open {
                    self.show_sidebar = true;
                    self.config.show_sidebar = true;
                }
                self.config.show_files_panel = self.panels.files;
                self.config.show_projects_panel = self.panels.projects;
                self.config_dirty = true;
                Task::none()
            }
            Message::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.config.show_sidebar = self.show_sidebar;
                self.config_dirty = true;
                Task::none()
            }
            Message::GitShortcut(what) => {
                // The panel opens first: what happens shows there.
                let open = if self.git.open {
                    Task::none()
                } else {
                    self.handle(Message::ToggleGit)
                };
                self.git.tab = crate::git_panel::Tab::Commit;
                let action = match what {
                    GitShortcut::Commit => Task::none(),
                    GitShortcut::Push => {
                        Task::done(Message::Git(crate::git_panel::GitMsg::PushCurrent))
                    }
                    GitShortcut::Pull => {
                        Task::done(Message::Git(crate::git_panel::GitMsg::RefAction(
                            crate::git_panel::branches::RefAction::Pull,
                        )))
                    }
                };
                Task::batch([open, action])
            }
            Message::OpenBranchSwitcher => {
                if self.git.status.is_none() {
                    return Task::none();
                }
                self.overlay = ActiveOverlay::BranchSwitch(String::new());
                Task::batch([
                    self.git.load_refs(),
                    iced::widget::operation::focus(git_file::branch_input_id()),
                ])
            }
            Message::BranchSwitchFilter(text) => {
                if let ActiveOverlay::BranchSwitch(filter) = &mut self.overlay {
                    *filter = text;
                }
                Task::none()
            }
            Message::BranchSwitchChoose(name, create) => {
                self.overlay = ActiveOverlay::None;
                if name.is_empty() {
                    return Task::none();
                }
                self.git.switch_branch(name, create)
            }
            Message::SetAutoFetch(minutes) => {
                self.config.git_auto_fetch_minutes = minutes;
                self.config_dirty = true;
                Task::none()
            }
            Message::SetPullMode(mode) => {
                // Chosen behind the chevron of the Git panel: the modes close.
                self.git.branches.menu = None;
                self.git.pull_mode = mode;
                self.config.git_pull_mode = mode;
                self.config_dirty = true;
                Task::none()
            }
            Message::SetMergeMode(mode) => {
                self.git.branches.menu = None;
                self.git.merge_mode = mode;
                self.config.git_merge_mode = mode;
                self.config_dirty = true;
                Task::none()
            }
            Message::SidebarResizeStart => {
                self.sidebar_resizing = true;
                Task::none()
            }
            Message::SidebarResize(x) => {
                // The list starts after the bar, a gap from the window's edge
                // and a gap from the bar.
                let most = (self.config.window_width * 0.6).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                self.sidebar_width = (x - SIDEBAR_LEFT).clamp(SIDEBAR_MIN, most);
                Task::none()
            }
            Message::PanelResizeStart => {
                self.panel_resizing = true;
                self.panel_drag = None;
                Task::none()
            }
            Message::PanelResize(y) => {
                // Dragging up makes it taller: the height follows how far the
                // pointer went from where it was at the first move.
                let (y0, h0) = *self.panel_drag.get_or_insert((y, self.panel_height));
                let most = (self.config.window_height * 0.85).max(PANEL_MIN_HEIGHT);
                self.panel_height = (h0 + y0 - y).clamp(PANEL_MIN_HEIGHT, most);
                Task::none()
            }
            Message::PanelResizeEnd => {
                self.panel_resizing = false;
                self.panel_drag = None;
                Task::none()
            }
            Message::SidebarResizeEnd => {
                self.sidebar_resizing = false;
                self.config.sidebar_width = self.sidebar_width;
                self.config_dirty = true;
                Task::none()
            }
            Message::AskDeleteFile(path) => Task::perform(
                async move {
                    let tracked = is_tracked(&path).await;
                    (path, tracked)
                },
                |(path, tracked)| Message::DeleteAsked(path, tracked),
            ),
            Message::DeleteAsked(path, tracked) => {
                // Checked by default: a file git tracks is usually meant to go from it too.
                self.overlay = ActiveOverlay::ConfirmFileDelete(path, tracked.then_some(true));
                Task::none()
            }
            Message::ToggleDeleteFromGit(on) => {
                if let ActiveOverlay::ConfirmFileDelete(_, from_git @ Some(_)) = &mut self.overlay {
                    *from_git = Some(on);
                }
                Task::none()
            }
            Message::DeleteFile(path, from_git) => {
                self.overlay = ActiveOverlay::None;
                self.bookmarks.clear(&path);
                let close = self.close_path(path.clone());
                let delete = Task::perform(delete_file(path, from_git), Message::FileDeleted);
                Task::batch([close, delete])
            }
            Message::FileDeleted(result) => {
                match result {
                    Ok(true) => self.set_status(t!("status.file_deleted_git"), false),
                    Ok(false) => self.set_status(t!("status.file_deleted"), false),
                    Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
                }
                Task::done(Message::Git(crate::git_panel::GitMsg::Refresh))
            }
            Message::WindowResized(width, height) => {
                // Remembered, and written with the next autosave tick.
                if (self.config.window_width, self.config.window_height) != (width, height) {
                    self.config.window_width = width;
                    self.config.window_height = height;
                    self.config_dirty = true;
                }
                Task::none()
            }
            Message::TogglePreferences => {
                self.show_preferences = !self.show_preferences;
                Task::none()
            }
            Message::ToggleTheme => {
                let dark = !self.config.dark_mode;
                self.handle(Message::ThemeChanged(dark))
            }
            Message::ThemeChanged(dark) => {
                self.config.dark_mode = dark;
                self.config.save();
                Task::none()
            }
            Message::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::OpenFile(path) => self.activate(path),
                SidebarAction::CloseFile(path) => self.request_close(path),
                // The cross asks nothing, even with unsaved changes.
                SidebarAction::CloseNow(path) => self.close_path(path),
                // Only offered for the active file.
                SidebarAction::SaveFile => self.handle(Message::SaveFile),
                SidebarAction::SaveFileAs => self.start_save_as(self.current_file.clone()),
                SidebarAction::None => Task::none(),
            },
            Message::Search(SearchMessage::ReplaceAll) => {
                let old = crate::editor::buffer_text(&self.editor.content);
                match self.search.replace_all(&old) {
                    Ok(new_text) if new_text != old => {
                        self.push_undo();
                        self.undo_new_group = true;
                        self.is_dirty = true;
                        self.search.reset_matches();
                        // Minimal replacement: scroll and cursor survive.
                        self.replace_buffer_text(&new_text);
                    }
                    Ok(_) => self.search.reset_matches(),
                    Err(e) => self.search.set_error(e),
                }
                Task::none()
            }
            Message::Search(msg) => {
                let replacing = matches!(msg, SearchMessage::Replace);
                let before = replacing.then(|| self.snapshot());
                let probe = replacing.then(|| self.edit_probe());
                let command = self.search.update(msg, &mut self.editor.content);
                // Replacements are edits like any other: undoable and dirty.
                if let (Some(before), Some((count, top))) = (before, probe) {
                    if crate::editor::buffer_text(&self.editor.content) != before.text {
                        self.undo_stack.push_back(before);
                        trim_history(&mut self.undo_stack);
                        self.redo_stack.clear();
                        self.undo_new_group = true;
                        self.is_dirty = true;
                        self.shift_bookmarks(count, top);
                    }
                }
                command
            }
            Message::Preferences(msg) => {
                // A language's completion setting: its servers start again
                // with it.
                if let PreferencesMessage::CompletionOption(..) = &msg {
                    let _ = self.preferences.update(msg, &mut self.config);
                    self.config_dirty = true;
                    crate::lsp::set_completion_options(&self.config.completion_options);
                    self.completion_servers_restarted();
                    return Task::future(crate::lsp::restart_servers()).discard();
                }
                let servers_page = matches!(
                    msg,
                    PreferencesMessage::PageSelected(crate::preferences::PrefsPage::Servers)
                );
                if self.preferences.update(msg, &mut self.config) {
                    self.config_dirty = true;
                }
                if servers_page {
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(|| {
                                (crate::lsp_install::ready(), crate::lsp_install::removable())
                            })
                            .await
                        },
                        |servers| Message::ServersChecked(servers.unwrap_or_default()),
                    );
                }
                Task::none()
            }
            Message::About => {
                self.overlay = ActiveOverlay::About;
                Task::none()
            }
            Message::Quit => match self.save_session_now() {
                Ok(()) => {
                    self.config.save();
                    std::process::exit(0)
                }
                // Unsaved buffers live on in the session file; if it can't be
                // written, quitting would lose them.
                Err(e) => {
                    self.overlay = ActiveOverlay::ConfirmQuit(e);
                    Task::none()
                }
            },
            Message::QuitAnyway => {
                self.config.save();
                std::process::exit(0)
            }
            Message::ContextCopy => {
                if let Some(text) = self.editor.content.selection() {
                    return iced::clipboard::write(text);
                }
                Task::none()
            }
            Message::ContextCut => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(text) = self.editor.content.selection() {
                    self.push_undo();
                    self.undo_new_group = true;
                    self.edited(|app| {
                        app.editor
                            .content
                            .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                    });
                    self.is_dirty = true;
                    return iced::clipboard::write(text);
                }
                Task::none()
            }
            Message::ContextPaste => {
                if self.editing_blocked() {
                    return Task::none();
                }
                iced::clipboard::read().map(Message::ClipboardContent)
            }
            Message::ClipboardContent(Some(text)) => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if text.len() > MAX_PASTE_BYTES {
                    self.set_status(
                        format!(
                            "{}: {}",
                            t!("status.error"),
                            t!("status.err_paste_too_large")
                        ),
                        true,
                    );
                    return Task::none();
                }
                self.push_undo();
                self.undo_new_group = true;
                self.edited(|app| {
                    app.editor.content.perform(text_editor::Action::Edit(
                        text_editor::Edit::Paste(std::sync::Arc::new(text)),
                    ));
                });
                self.is_dirty = true;
                Task::none()
            }
            Message::ClipboardContent(None) => Task::none(),
            Message::ContextDelete => {
                // Nothing selected, nothing to delete (and no empty undo step).
                if self.editing_blocked() || self.editor.content.selection().is_none() {
                    return Task::none();
                }
                self.push_undo();
                self.undo_new_group = true;
                self.edited(|app| {
                    app.editor
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                });
                self.is_dirty = true;
                Task::none()
            }
            Message::FormatFile => {
                if self.is_formatting {
                    return Task::none();
                }
                if let Some(ext) = self.editor.language.clone() {
                    self.is_formatting = true;
                    self.set_status(t!("status.formatting"), false);
                    let content = crate::editor::buffer_text(&self.editor.content);
                    let job = FormatJob {
                        path: self.current_file.clone(),
                        source: fingerprint(&content),
                    };
                    let target = self.format_target();
                    Task::perform(
                        crate::formatter::format(content, ext, target),
                        move |result| Message::FileFormatted(job.clone(), result),
                    )
                } else {
                    self.set_status(t!("status.no_language"), false);
                    Task::none()
                }
            }
            Message::FormatSelection => {
                if self.is_formatting {
                    return Task::none();
                }
                if let (Some(ext), Some(selected)) = (
                    self.editor.language.clone(),
                    self.editor.content.selection(),
                ) {
                    self.is_formatting = true;
                    self.set_status(t!("status.formatting"), false);
                    let job = FormatJob {
                        path: self.current_file.clone(),
                        source: fingerprint(&selected),
                    };
                    let target = self.format_target();
                    // Formatters end their output with a newline; a fragment
                    // that didn't end with one shouldn't gain one.
                    let had_newline = selected.ends_with('\n');
                    Task::perform(
                        async move {
                            crate::formatter::format(selected, ext, target)
                                .await
                                .map(|f| {
                                    if had_newline {
                                        f
                                    } else {
                                        f.trim_end_matches('\n').to_string()
                                    }
                                })
                        },
                        move |result| Message::SelectionFormatted(job.clone(), result),
                    )
                } else {
                    Task::none()
                }
            }
            Message::FileFormatted(job, result) => {
                self.is_formatting = false;
                // The user moved on, or kept typing: the result describes text
                // that is no longer there.
                let current = self.current_file == job.path
                    && fingerprint(&crate::editor::buffer_text(&self.editor.content)) == job.source;
                if !current {
                    self.set_status(t!("status.format_stale"), true);
                    return Task::none();
                }
                match result {
                    Ok(formatted) => {
                        let (line, col) = self.editor.content.cursor_position();
                        self.format_error = None;
                        self.show_error_panel = false;
                        self.set_status(t!("status.formatted"), false);
                        let formatted = TextFormat::decode(&formatted).0;
                        return self.apply_text_edit(formatted, line, col, None);
                    }
                    Err(e) => self.show_format_error(e),
                }
                Task::none()
            }
            Message::SelectionFormatted(job, result) => {
                self.is_formatting = false;
                let current = self.current_file == job.path
                    && self
                        .editor
                        .content
                        .selection()
                        .is_some_and(|sel| fingerprint(&sel) == job.source);
                if !current {
                    self.set_status(t!("status.format_stale"), true);
                    return Task::none();
                }
                match result {
                    Ok(formatted) => {
                        self.push_undo();
                        self.undo_new_group = true;
                        self.edited(|app| {
                            app.editor.content.perform(text_editor::Action::Edit(
                                text_editor::Edit::Paste(std::sync::Arc::new(formatted)),
                            ));
                        });
                        self.is_dirty = true;
                        self.format_error = None;
                        self.show_error_panel = false;
                        self.set_status(t!("status.formatted"), false);
                    }
                    Err(e) => self.show_format_error(e),
                }
                Task::none()
            }
            Message::CloseErrorPanel => {
                self.show_error_panel = false;
                Task::none()
            }
            Message::Undo if self.on_diff_tab() || self.panel_diff_focused() => {
                self.undo_work(true)
            }
            Message::Redo if self.on_diff_tab() || self.panel_diff_focused() => {
                self.undo_work(false)
            }
            Message::Undo if self.on_merge_tab() => {
                self.merge_msg(crate::git_panel::merge::MergeMsg::Undo)
            }
            Message::Redo if self.on_merge_tab() => {
                self.merge_msg(crate::git_panel::merge::MergeMsg::Redo)
            }
            Message::Undo => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(prev) = self.undo_stack.pop_back() {
                    let current = self.snapshot();
                    self.redo_stack.push_back(current);
                    trim_history(&mut self.redo_stack);
                    return self.restore_snapshot(prev);
                }
                Task::none()
            }
            Message::Redo => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(next) = self.redo_stack.pop_back() {
                    let current = self.snapshot();
                    self.undo_stack.push_back(current);
                    trim_history(&mut self.undo_stack);
                    return self.restore_snapshot(next);
                }
                Task::none()
            }
            Message::AutoSave => {
                self.flush_scratch();
                if self.config_dirty {
                    self.config_dirty = false;
                    self.config.save();
                }
                // The tree follows what happens on the disk, and its colors
                // what Git says (now and then).
                self.projects.refresh(false);
                let states = if self.ticks.is_multiple_of(3) {
                    self.refresh_file_states()
                } else {
                    Task::none()
                };
                let external = self.check_external_change();
                self.ticks = self.ticks.wrapping_add(1);
                // The marks follow HEAD, which a commit made elsewhere moves.
                let marks = if self.ticks.is_multiple_of(10) {
                    self.refresh_marks_base()
                } else {
                    self.refresh_marks_now();
                    Task::none()
                };
                // With the Git panel closed, the status is still looked at now
                // and then, to notice conflicts.
                let probe = if !self.git.open
                    && self.ticks.is_multiple_of(5)
                    && self.current_file.as_deref().is_some_and(|p| !is_virtual(p))
                {
                    Task::done(Message::Git(crate::git_panel::GitMsg::Tick))
                } else {
                    Task::none()
                };
                // What the remotes have is looked at now and then, in the background.
                let minutes = u64::from(self.config.git_auto_fetch_minutes);
                let fetch = if minutes > 0
                    && (self.ticks as u64).is_multiple_of(minutes * 60)
                    && self.git.repo.is_some()
                {
                    Task::done(Message::Git(crate::git_panel::GitMsg::AutoFetch))
                } else {
                    Task::none()
                };
                let external = Task::batch([
                    external,
                    self.auto_save_files(),
                    marks,
                    probe,
                    fetch,
                    states,
                ]);
                if self.rev == self.session_rev {
                    return external;
                }
                match self.session_write_job() {
                    Some(job) => {
                        Task::batch([external, Task::perform(job, Message::SessionWritten)])
                    }
                    None => external,
                }
            }
            Message::SessionWritten(result) => {
                match result {
                    Ok(()) => {}
                    Err(e) => {
                        // Try again at the next tick.
                        self.last_session = 0;
                        self.session_rev = 0;
                        self.set_status(format!("{}: {}", t!("status.session_not_saved"), e), true);
                    }
                }
                Task::none()
            }
            Message::PreviewTick => {
                self.refresh_preview();
                Task::none()
            }
            // --- Overlays ---
            Message::CloseOverlay => {
                if self.overlay_is_dismissable() {
                    self.capturing = None;
                    self.overlay = ActiveOverlay::None;
                }
                Task::none()
            }
            Message::OpenGotoLine => {
                self.goto_line_input = String::new();
                self.overlay = ActiveOverlay::GotoLine;
                iced::widget::operation::focus(goto_line_id())
            }
            Message::GotoLineInputChanged(s) => {
                // Allow only digits
                self.goto_line_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }
            Message::GotoLineSubmit => {
                if let Ok(n) = self.goto_line_input.parse::<usize>() {
                    let max = self.editor.content.line_count().saturating_sub(1);
                    let target = n.saturating_sub(1).min(max);
                    self.remember_place();
                    crate::search::goto_line_start(&mut self.editor.content, target);
                }
                self.overlay = ActiveOverlay::None;
                Task::none()
            }
            Message::OpenLanguagePicker => {
                self.overlay = ActiveOverlay::LanguagePicker;
                Task::none()
            }
            Message::SetLanguage(ext) => {
                self.editor.language = if ext.is_empty() { None } else { Some(ext) };
                self.reset_view_mode();
                self.overlay = ActiveOverlay::None;
                Task::none()
            }
            Message::OpenShortcuts => {
                self.capturing = None;
                self.overlay = ActiveOverlay::Shortcuts;
                Task::none()
            }
            Message::StartCaptureShortcut(target) => {
                self.capturing = Some(target);
                Task::none()
            }
            Message::OpenPalette => self.open_everywhere(crate::everywhere::Tab::Actions),
            Message::OpenEverywhere(tab) => self.open_everywhere(tab),
            Message::DoubleShift => {
                if self.capturing.is_some() {
                    return Task::none();
                }
                if self.overlay == ActiveOverlay::Everywhere {
                    return self.everywhere_non_project();
                }
                if !matches!(self.overlay, ActiveOverlay::None) && !self.overlay_is_dismissable() {
                    return Task::none();
                }
                self.open_everywhere(crate::everywhere::Tab::All)
            }
            Message::EverywhereQuery(query) => self.everywhere_typed(query),
            Message::EverywhereTab(tab) => self.everywhere_switch(tab),
            Message::EverywhereOpen(pick) => self.everywhere_open(pick),
            Message::EverywhereFiles(generation, files) => self.everywhere_files(generation, files),
            Message::EverywhereSymbolsDue(generation) => self.everywhere_symbols_due(generation),
            Message::EverywhereSymbols(generation, found) => {
                self.everywhere_symbols(generation, found)
            }
            Message::EverywhereTextDue(generation) => self.everywhere_text_due(generation),
            Message::EverywhereText(generation, found) => {
                self.everywhere_text(generation, found);
                Task::none()
            }
            Message::EverywhereGit(branches, commits) => {
                self.everywhere_git(branches, commits);
                Task::none()
            }
            Message::EverywhereNonProject => self.everywhere_non_project(),
            Message::EverywhereFilter => {
                self.everywhere.filter_open = !self.everywhere.filter_open;
                Task::none()
            }
            Message::EverywhereFilterType(kind, on) => self.everywhere_filter_type(kind, on),
            Message::EverywhereTextOption(option) => self.everywhere_text_option(option),
            Message::EverywhereExport => self.everywhere_export(),
            Message::EverywhereAbbreviate(command) => self.everywhere_abbreviate(command),
            Message::EverywhereAbbreviationTyped(text) => {
                if let Some((_, typed)) = self.everywhere.abbreviating.as_mut() {
                    *typed = text;
                }
                Task::none()
            }
            Message::EverywhereAbbreviationSubmit => self.everywhere_abbreviation_submit(),
            Message::EverywhereAbbreviationRemove(command, abbreviation) => {
                self.everywhere_abbreviation_remove(command, abbreviation);
                Task::none()
            }
            Message::EverywhereAssign(command) => self.everywhere_assign(command),
            // Raw key forwarded from the subscription fn pointer
            Message::KeyPressed {
                key,
                ctrl,
                shift,
                alt,
            } => {
                // In shortcut capture mode: save the new binding
                if let Some(target) = self.capturing {
                    let sc = ShortcutConfig {
                        ctrl,
                        shift,
                        alt,
                        key,
                    };
                    // A bare letter (or Shift+letter) would fire on every
                    // keystroke typed into the editor.
                    if !(sc.ctrl || sc.alt || crate::config::is_function_key(&sc.key)) {
                        let ctrl = if cfg!(target_os = "macos") {
                            "⌘"
                        } else {
                            "Ctrl"
                        };
                        self.set_status(t!("shortcuts.needs_modifier", ctrl = ctrl), true);
                        return Task::none();
                    }
                    if crate::code_editor::consumes_key(&sc.key, sc.ctrl, sc.alt) {
                        self.set_status(t!("shortcuts.reserved"), true);
                        return Task::none();
                    }
                    if self.config.shortcuts.conflicts(target, &sc) {
                        self.set_status(t!("shortcuts.conflict"), true);
                        return Task::none();
                    }
                    *self.config.shortcuts.slot_mut(target) = sc;
                    self.config.save();
                    self.capturing = None;
                    return Task::none();
                }
                // Typing a commit message: the app's shortcuts stay out of it.
                if self.git.typing {
                    // Going back in a merge result is still the user's own.
                    if self.on_merge_tab() && ctrl && key.eq_ignore_ascii_case("z") {
                        return self.handle(if shift { Message::Redo } else { Message::Undo });
                    }
                    if self.on_merge_tab() && ctrl && key.eq_ignore_ascii_case("y") {
                        return self.handle(Message::Redo);
                    }
                    let toggle = &self.config.shortcuts.toggle_git;
                    if toggle.key.eq_ignore_ascii_case(&key)
                        && toggle.ctrl == ctrl
                        && toggle.alt == alt
                        && toggle.shift == shift
                    {
                        return self.handle(Message::ToggleGit);
                    }
                    return Task::none();
                }
                // The terminal has the keyboard: it gets every key (it reads
                // them itself), except the one that hides it.
                if self.terminal.focused {
                    let toggle = &self.config.shortcuts.toggle_terminal;
                    if toggle.key.eq_ignore_ascii_case(&key)
                        && toggle.ctrl == ctrl
                        && toggle.alt == alt
                        && toggle.shift == shift
                    {
                        return self.handle(Message::ToggleTerminal);
                    }
                    return Task::none();
                }
                // The list of completions: its arrows, pages, Enter and Tab.
                if self.completion_visible() && !ctrl && !alt {
                    if let Some(task) = self.completion_key(&key) {
                        return task;
                    }
                }
                // Alt+↑ / Alt+↓: another signature of an overloaded call.
                if alt
                    && !ctrl
                    && !shift
                    && (key == "ArrowUp" || key == "ArrowDown")
                    && self.hint_cycle(if key == "ArrowDown" { 1 } else { -1 })
                {
                    return Task::none();
                }
                // Tab in a snippet: its next place.
                if key == "Tab" && !ctrl && !alt && !shift && self.snippet_next() {
                    return Task::none();
                }
                // Search everywhere: its keys; the shortcut of a tab goes to
                // that tab (again: beyond the project).
                if self.overlay == ActiveOverlay::Everywhere {
                    if let Some(task) = self.everywhere_key(&key, ctrl, shift, alt) {
                        return task;
                    }
                    let sh = &self.config.shortcuts;
                    let is = |s: &ShortcutConfig| {
                        s.key.eq_ignore_ascii_case(&key)
                            && s.ctrl == ctrl
                            && s.alt == alt
                            && s.shift == shift
                    };
                    use crate::everywhere::Tab;
                    let tab = [
                        (&sh.goto_file, Tab::Files),
                        (&sh.goto_class, Tab::Classes),
                        (&sh.goto_symbol, Tab::Symbols),
                        (&sh.command_palette, Tab::Actions),
                    ]
                    .into_iter()
                    .find(|(s, _)| is(s))
                    .map(|(_, t)| t);
                    if let Some(tab) = tab {
                        return self.open_everywhere(tab);
                    }
                    return Task::none();
                }
                // The finder cards: arrows move in the list.
                if self.overlay == ActiveOverlay::FindInFiles {
                    return match (key.as_str(), ctrl, alt) {
                        // Alt+↓: the searches made.
                        ("ArrowDown", false, true) => {
                            self.handle(Message::FindMenu(Some(finder::Menu::History)))
                        }
                        // On several lines the arrows are the text's:
                        // Ctrl+Alt+↑/↓ go through the matches.
                        ("ArrowDown", true, true) => self.finder_step(1),
                        ("ArrowUp", true, true) => self.finder_step(-1),
                        ("ArrowDown", false, false) if !self.finder.multiline => {
                            self.finder_step(1)
                        }
                        ("ArrowUp", false, false) if !self.finder.multiline => self.finder_step(-1),
                        // Ctrl+Enter: to the Results panel.
                        ("Return", true, false) => self.finder_to_window(),
                        _ => Task::none(),
                    };
                }
                // The usages list: arrows move, Enter opens the highlighted one.
                if let ActiveOverlay::References(list) = &mut self.overlay {
                    match key.as_str() {
                        "ArrowDown" | "ArrowUp" => {
                            let delta = if key == "ArrowDown" { 1 } else { -1 };
                            list.selected =
                                crate::palette::step(list.selected, delta, list.items.len());
                            return views::scroll_references_to(list.selected);
                        }
                        "Return" => {
                            let selected = list.selected;
                            return self.handle(Message::OpenReference(selected));
                        }
                        _ => {}
                    }
                }
                // Don't fire shortcuts while any overlay or the Preferences
                // window is open
                if !matches!(self.overlay, ActiveOverlay::None) || self.show_preferences {
                    return Task::none();
                }
                // Ctrl+Alt+↑/↓: the previous or next match of the Results.
                if ctrl
                    && alt
                    && !shift
                    && (key == "ArrowUp" || key == "ArrowDown")
                    && !self.results.tabs.is_empty()
                {
                    return self.results_update(results::ResultsMsg::Step(key == "ArrowDown"));
                }
                // The Results panel, clicked last: its keys.
                if let Some(task) = self.results_key(&key, ctrl, shift, alt) {
                    return task;
                }
                // The tree, clicked last: the arrows move in it, Enter opens.
                if self.tree_focus && !ctrl && !alt && self.show_sidebar && self.panels.projects {
                    match key.as_str() {
                        "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => {
                            self.projects.step(&key);
                            self.save_projects();
                            return self.scroll_tree_to_selected();
                        }
                        "Return" => {
                            let Some(path) = self.projects.selected.clone() else {
                                return Task::none();
                            };
                            if path.is_dir() {
                                self.projects.toggle(&path);
                                self.save_projects();
                                return Task::none();
                            }
                            // Enter keeps the file open, as a second click does.
                            if self.preview_tab.as_ref() == Some(&path) {
                                self.preview_tab = None;
                            }
                            return self.activate(path);
                        }
                        _ => {}
                    }
                }
                // On a diff, Ctrl+A picks every row and Ctrl+C / Ctrl+X copy the
                // picked ones (a diff can't be cut from).
                if ctrl && !alt && !shift {
                    if let Some(task) = self.diff_clipboard(&key) {
                        return task;
                    }
                }
                // Enter in the search card goes to the next match, Shift+Enter
                // to the previous one — when the find field has the focus
                // (it captures the key itself, which is why this listens to
                // captured events too).
                if key == "Return" && !ctrl && !alt {
                    if self.show_search {
                        return iced::advanced::widget::operate(crate::search::find_field_focused()).map(
                            move |in_find_field| Message::SearchEnterChecked {
                                shift,
                                in_find_field,
                            },
                        );
                    }
                    return Task::none();
                }
                // Tab indents, Shift+Tab dedents; a search field keeps it.
                if key == "Tab" {
                    // Ctrl+Tab goes to the tab seen before (again for older
                    // ones while Ctrl is held), Ctrl+Shift+Tab back.
                    if ctrl && !alt {
                        return self.switch_tab(!shift);
                    }
                    if ctrl || alt {
                        return Task::none();
                    }
                    let m = if shift {
                        Message::DedentLine
                    } else {
                        Message::InsertTab
                    };
                    return self.check_search_focus_then(m);
                }
                // Match against config shortcuts. A symbol key (`/`) may need
                // Shift on some layouts (AZERTY), so Shift doesn't count
                // against a binding that doesn't use it.
                let symbol_key = key.chars().count() == 1
                    && key.chars().next().is_some_and(|c| !c.is_alphanumeric());
                let sc_check = |s: &ShortcutConfig| -> bool {
                    s.key.eq_ignore_ascii_case(&key)
                        && s.ctrl == ctrl
                        && s.alt == alt
                        && (s.shift == shift || (symbol_key && !s.shift))
                };
                let sh = &self.config.shortcuts;
                // Order matters: Ctrl+Shift+S must be tried before Ctrl+S.
                let bindings: [(&ShortcutConfig, Message); 39] = [
                    (&sh.complete, Message::CompletionInvoke),
                    (&sh.parameter_info, Message::HintInvoke),
                    (&sh.toggle_git, Message::ToggleGit),
                    (&sh.git_commit, Message::GitShortcut(GitShortcut::Commit)),
                    (&sh.git_push, Message::GitShortcut(GitShortcut::Push)),
                    (&sh.git_pull, Message::GitShortcut(GitShortcut::Pull)),
                    (&sh.git_branches, Message::OpenBranchSwitcher),
                    (&sh.bookmark_panel, Message::ToggleBookmarkPanel),
                    (&sh.toggle_terminal, Message::ToggleTerminal),
                    (&sh.nav_back, Message::NavBack),
                    (&sh.nav_forward, Message::NavForward),
                    (&sh.undo, Message::Undo),
                    (&sh.redo, Message::Redo),
                    (&sh.duplicate_line, Message::DuplicateLine),
                    (&sh.move_line_up, Message::MoveLineUp),
                    (&sh.move_line_down, Message::MoveLineDown),
                    (&sh.toggle_comment, Message::ToggleComment),
                    (&sh.delete_line, Message::DeleteLine),
                    (&sh.toggle_bookmark, Message::ToggleBookmark),
                    (&sh.prev_bookmark, Message::PrevBookmark),
                    (&sh.next_bookmark, Message::NextBookmark),
                    (&sh.toggle_fold, Message::ToggleFold),
                    (&sh.save_as, Message::SaveFileAs),
                    (&sh.new_file, Message::NewFile),
                    (&sh.save_file, Message::SaveFile),
                    (&sh.open_file, Message::OpenFile),
                    (&sh.close_file, Message::CloseFile),
                    (&sh.find, Message::ToggleSearch),
                    (&sh.select_all, Message::SelectAll),
                    (&sh.format_code, Message::FormatFile),
                    (&sh.goto_line, Message::OpenGotoLine),
                    (&sh.command_palette, Message::OpenPalette),
                    (&sh.toggle_sidebar, Message::ToggleSidebar),
                    (&sh.goto_file, Message::OpenGotoFile),
                    (
                        &sh.goto_class,
                        Message::OpenEverywhere(crate::everywhere::Tab::Classes),
                    ),
                    (
                        &sh.goto_symbol,
                        Message::OpenEverywhere(crate::everywhere::Tab::Symbols),
                    ),
                    (&sh.find_in_files, Message::OpenFindInFiles),
                    (&sh.replace_in_files, Message::OpenReplaceInFiles),
                    (&sh.reopen_tab, Message::ReopenTab),
                ];
                let msg = bindings
                    .into_iter()
                    .find(|(s, _)| sc_check(s))
                    .map(|(_, m)| m)
                    .or_else(|| sc_check(&sh.quit).then_some(Message::Quit));
                match msg {
                    Some(m) => self.check_search_focus_then(m),
                    None => Task::none(),
                }
            }
            Message::DuplicateLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::duplicate_lines(&text, first, last);
                // The copy goes below, and the cursor follows it.
                let shift = (last - first + 1) as isize;
                self.apply_line_edit(&text, new_text, shift, span)
            }
            Message::MoveLineUp => self.move_lines(true),
            Message::MoveLineDown => self.move_lines(false),
            Message::ToggleComment => {
                if self.editing_blocked() {
                    return Task::none();
                }
                // Plain text and formats without comments (JSON) have nothing
                // to toggle.
                let Some((prefix, suffix)) = comment_style(self.editor.language.as_deref()) else {
                    return Task::none();
                };
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::toggle_comment_lines(&text, first, last, prefix, suffix);
                self.apply_line_edit(&text, new_text, 0, span)
            }
            Message::DeleteLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (first, last) = self.lines_to_edit(self.selection_span());
                let (new_text, new_line) = text_ops::delete_lines(&text, first, last);
                self.apply_text_edit(new_text, new_line, 0, None)
            }
            Message::ToggleBookmark => {
                let Some(path) = self.current_file.clone() else {
                    return Task::none();
                };
                let (line, _) = self.editor.content.cursor_position();
                let added = self.bookmarks.toggle(&path, line);
                self.status_message = if added {
                    t!("status.bookmark_added")
                } else {
                    t!("status.bookmark_removed")
                }
                .to_string();
                self.status_is_error = false;
                self.server_offer = None;
                Task::none()
            }
            Message::NavBack => self.navigate(false),
            Message::NavForward => self.navigate(true),
            Message::ToggleFold => {
                self.fold(crate::code_editor::FoldCommand::AtCursor);
                Task::none()
            }
            Message::FoldAll => {
                self.fold(crate::code_editor::FoldCommand::All(true));
                Task::none()
            }
            Message::UnfoldAll => {
                self.fold(crate::code_editor::FoldCommand::All(false));
                Task::none()
            }
            Message::FoldClick(line, scope) => {
                self.fold(crate::code_editor::FoldCommand::Line(line, scope));
                Task::none()
            }
            Message::EditorRightClick(at) => {
                let annotated = self.blame_shown();
                let origin = crate::editor::text_origin(
                    &self.config,
                    self.editor.content.line_count(),
                    annotated,
                );
                self.gutter_click = self.config.show_line_numbers && at.x < origin.x;
                self.gutter_line = self
                    .gutter_click
                    .then(|| self.editor.content.line_at_y(at.y - origin.y))
                    .flatten();
                // The context menu acts on the clicked line: move the caret
                // there, unless there is a selection to act on.
                if self.editor.content.selection().is_none() && !self.gutter_click {
                    self.editor
                        .content
                        .perform(text_editor::Action::Click(at - origin));
                    self.undo_new_group = true;
                }
                Task::none()
            }
            Message::ChangeBar(line) => {
                let Some(path) = self.current_file.clone() else {
                    return Task::none();
                };
                let same = self
                    .revert
                    .as_ref()
                    .is_some_and(|(p, l, r)| *p == path && *l == line && *r == self.buffer_stamp());
                self.revert = (!same).then(|| (path, line, self.buffer_stamp()));
                Task::none()
            }
            Message::CloseConflictToast => {
                self.conflict_toast_closed = true;
                Task::none()
            }
            Message::CloseRevert => {
                self.revert = None;
                Task::none()
            }
            Message::RevertChange => {
                let Some((path, line, _)) = self.revert.take() else {
                    return Task::none();
                };
                if self.editing_blocked() || self.current_file.as_ref() != Some(&path) {
                    return Task::none();
                }
                let base = self.marks.as_ref().and_then(|m| m.base.clone());
                let old = crate::editor::buffer_text(&self.editor.content);
                match crate::git::marks::revert(base.as_deref(), &old, line) {
                    Some(new_text) => self.apply_line_edit(&old, new_text, 0, None),
                    None => Task::none(),
                }
            }
            Message::GotoDefinition => self.goto_definition(),
            Message::OpenReference(index) => {
                let ActiveOverlay::References(list) = &self.overlay else {
                    return Task::none();
                };
                let Some(hit) = list.items.get(index).map(|r| r.hit.clone()) else {
                    return Task::none();
                };
                self.overlay = ActiveOverlay::None;
                self.jump_to(hit)
            }
            Message::InstallServer(binary) => {
                if !self.servers.installing.insert(binary) {
                    return Task::none();
                }
                self.set_status(t!("status.server_installing", name = binary), false);
                Task::perform(crate::lsp_install::install(binary), move |result| {
                    Message::ServerInstalled(binary, result)
                })
            }
            Message::ServerInstalled(binary, result) => {
                self.servers.installing.remove(binary);
                match result {
                    Ok(()) => {
                        self.servers.ready.insert(binary);
                        self.servers.removable.insert(binary);
                        self.set_status(t!("status.server_installed", name = binary), false);
                    }
                    Err(error) => {
                        self.set_status(
                            t!("status.server_install_failed", name = binary, error = error),
                            true,
                        );
                    }
                }
                Task::none()
            }
            Message::ServersChecked((ready, removable)) => {
                self.servers.ready = ready;
                self.servers.removable = removable;
                Task::none()
            }
            Message::UninstallServer(binary) => {
                if !self.servers.removing.insert(binary) {
                    return Task::none();
                }
                Task::perform(crate::lsp_install::uninstall(binary), move |result| {
                    Message::ServerUninstalled(binary, result)
                })
            }
            Message::ServerUninstalled(binary, result) => {
                self.servers.removing.remove(binary);
                match result {
                    Ok(()) => {
                        self.servers.ready.remove(binary);
                        self.servers.removable.remove(binary);
                        self.set_status(t!("status.server_uninstalled", name = binary), false);
                    }
                    Err(error) => {
                        let (name, why) = (binary, error);
                        let m = t!("status.server_uninstall_failed", name = name, error = why);
                        self.set_status(m, true);
                    }
                }
                Task::none()
            }
            Message::DefinitionFound(word, outcome) => {
                use crate::lsp::Outcome;
                // Closed (or replaced) while searching: the answer is dropped.
                let waiting = matches!(
                    &self.overlay,
                    ActiveOverlay::References(r) if r.state == RefState::Searching && r.word == word
                );
                if !waiting {
                    return Task::none();
                }
                match outcome {
                    Outcome::Found(hit) => {
                        self.overlay = ActiveOverlay::None;
                        self.jump_to(hit)
                    }
                    Outcome::References(items) => {
                        self.overlay = ActiveOverlay::References(RefList {
                            word,
                            items,
                            selected: 0,
                            state: RefState::Ready,
                        });
                        Task::none()
                    }
                    other => {
                        let message = match other {
                            Outcome::NotFound => t!("status.no_definition", name = word),
                            Outcome::AtDefinition => t!("status.no_references", name = word),
                            Outcome::Failed(error) => {
                                t!("status.language_server_error", error = error)
                            }
                            // Answered by the pattern search instead.
                            Outcome::Found(_)
                            | Outcome::References(_)
                            | Outcome::Unsupported(_)
                            | Outcome::NoServer(..) => {
                                return Task::none();
                            }
                        };
                        self.overlay = ActiveOverlay::References(RefList {
                            word,
                            items: Vec::new(),
                            selected: 0,
                            state: RefState::Empty(message.to_string()),
                        });
                        Task::none()
                    }
                }
            }
            Message::NextBookmark => self.jump_to_bookmark(true),
            Message::PrevBookmark => self.jump_to_bookmark(false),
            Message::ClearBookmarks => {
                if let Some(path) = self.current_file.clone() {
                    self.bookmarks.clear(&path);
                    self.set_status(t!("status.bookmarks_cleared"), false);
                }
                Task::none()
            }
            Message::IndentSelection => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let Some(span) = self.selection_span() else {
                    return Task::none();
                };
                let (first, last) = span.lines();
                let text = crate::editor::buffer_text(&self.editor.content);
                let new_text = text_ops::indent_lines(&text, first, last, &self.indent_unit());
                self.apply_line_edit(&text, new_text, 0, Some(span))
            }
            Message::DedentLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::dedent_lines(
                    &text,
                    first,
                    last,
                    &self.indent_unit(),
                    self.config.tab_width,
                );
                self.apply_line_edit(&text, new_text, 0, span)
            }
            Message::OpenRecentFile(path) => {
                if !path.exists() {
                    self.config
                        .recent_files
                        .retain(|f| *f != path.to_string_lossy().as_ref());
                    self.config.save();
                    self.set_status(
                        format!("{}: {}", t!("status.error"), t!("status.no_file")),
                        true,
                    );
                    return Task::none();
                }
                // The tab appears once the file has been read.
                if self.sidebar.files().contains(&path) {
                    self.activate(path)
                } else {
                    Task::perform(read_file(path, false), Message::FileOpened)
                }
            }
            Message::ShortcutChecked(inner, focused) => {
                if focused {
                    Task::none()
                } else {
                    self.handle(*inner)
                }
            }
            Message::SearchEnter { shift } => self.handle(Message::Search(if shift {
                SearchMessage::FindPrevious
            } else {
                SearchMessage::Find
            })),
            Message::SearchEnterChecked {
                shift,
                in_find_field,
            } => {
                if in_find_field {
                    self.handle(Message::SearchEnter { shift })
                } else {
                    Task::none()
                }
            }
            Message::EscapePressed => {
                // Innermost first: the list of completions, a pending
                // rebind, a dialog, the Preferences window, the search card.
                self.completion.snippet = None;
                if self.overlay == ActiveOverlay::FindInFiles
                    && (self.finder.menu.is_some() || self.finder.confirm_all)
                {
                    self.finder.menu = None;
                    self.finder.confirm_all = false;
                } else if self.overlay == ActiveOverlay::Everywhere
                    && (self.everywhere.abbreviating.is_some() || self.everywhere.filter_open)
                {
                    self.everywhere.abbreviating = None;
                    self.everywhere.filter_open = false;
                } else if self.completion_visible() {
                    self.completion_close();
                } else if self.completion.hint.is_some() {
                    self.completion.hint = None;
                } else if self.capturing.is_some() {
                    self.capturing = None;
                } else if !matches!(self.overlay, ActiveOverlay::None) {
                    if self.overlay_is_dismissable() {
                        self.overlay = ActiveOverlay::None;
                    }
                } else if self.show_preferences {
                    self.show_preferences = false;
                } else if self.show_search {
                    self.show_search = false;
                }
                Task::none()
            }
            Message::CheckForUpdate => {
                self.overlay = ActiveOverlay::UpdateChecking;
                Task::perform(crate::updater::check_for_update(), Message::UpdateChecked)
            }
            Message::OpenChangelog => {
                self.overlay = ActiveOverlay::Changelog;
                Task::none()
            }
            Message::UpdateChecked(Ok(Some((version, notes)))) => {
                self.overlay = ActiveOverlay::UpdateAvailable(version, notes);
                Task::none()
            }
            Message::UpdateChecked(Ok(None)) => {
                self.overlay = ActiveOverlay::UpdateUpToDate;
                Task::none()
            }
            Message::UpdateChecked(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::ConfirmInstallUpdate(version) => {
                self.overlay = ActiveOverlay::UpdateDownloading;
                Task::perform(
                    crate::updater::download_update(version),
                    Message::UpdateDownloaded,
                )
            }
            Message::UpdateDownloaded(Ok((path, hash))) => {
                self.overlay = ActiveOverlay::UpdateInstalling;
                Task::perform(
                    crate::updater::install_update(path, hash),
                    Message::UpdateInstalled,
                )
            }
            Message::UpdateDownloaded(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::UpdateInstalled(Ok(())) => {
                if let Err(e) = self.save_session_now() {
                    self.overlay = ActiveOverlay::UpdateError(e);
                    return Task::none();
                }
                self.config.save();
                // On success this never returns — the process re-execs and exits.
                if let Err(e) = crate::updater::restart() {
                    self.overlay = ActiveOverlay::UpdateError(e);
                }
                Task::none()
            }
            Message::UpdateInstalled(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                Task::none()
            }
            Message::JsonToggle(path) => {
                // Ctrl+click opens every node, Alt+click closes every node.
                if self.mod_ctrl {
                    self.handle(Message::JsonExpandAll)
                } else if self.mod_alt {
                    self.handle(Message::JsonCollapseAll)
                } else {
                    let collapsed = std::sync::Arc::make_mut(&mut self.json_collapsed);
                    if !collapsed.remove(&path) {
                        collapsed.insert(path);
                    }
                    self.preview_rev += 1;
                    Task::none()
                }
            }
            Message::JsonExpandAll => {
                self.json_collapsed = Default::default();
                self.preview_rev += 1;
                Task::none()
            }
            Message::JsonCollapseAll => {
                if let PreviewDoc::Json(value) = &self.preview {
                    self.json_collapsed =
                        std::sync::Arc::new(crate::preview::json::container_paths(value));
                    self.preview_rev += 1;
                }
                Task::none()
            }
            Message::ModifiersChanged { ctrl, alt, shift } => {
                // Ctrl let go: the tab reached is the one seen last.
                if !ctrl {
                    self.tab_switch = None;
                }
                self.mod_ctrl = ctrl;
                self.mod_alt = alt;
                self.mod_shift = shift;
                Task::none()
            }
            Message::MarkdownLinkClicked(href) => {
                crate::preview::markdown::find_offset_command(&href).map(Message::PreviewScrollTo)
            }
            Message::PreviewScrollTo(Some(y)) => iced::widget::operation::scroll_to(
                crate::preview::markdown::scrollable_id(),
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    // A little breathing room above the heading, clamped so
                    // targets near the top don't ask for a negative scroll.
                    y: (y - 12.0).max(0.0),
                },
            ),
            Message::PreviewScrollTo(None) => Task::none(),
        }
    }

    /// Asks the renderer where the open menu's button sits, so its dropdown
    /// can be anchored under it regardless of label/font/DPI.
    fn query_menu_bounds(&self) -> Task<Message> {
        match self.open_menu {
            Some(m) => iced::widget::selector::find(iced::widget::selector::id(
                crate::menu_bar::menu_id(m),
            ))
            .map(move |target| Message::MenuBounds(m, target.and_then(|t| t.visible_bounds()))),
            None => Task::none(),
        }
    }

    /// Whether the open overlay can be closed by hand.
    fn overlay_is_dismissable(&self) -> bool {
        !matches!(
            self.overlay,
            ActiveOverlay::UpdateDownloading | ActiveOverlay::UpdateInstalling
        )
    }

    /// Recomputes the selection label of the status bar.
    fn refresh_selection_info(&mut self) {
        let key = (
            self.editor.content.selection_range(),
            self.editor.content.revision(),
        );
        if self.selection_key == Some(key) {
            return;
        }
        self.selection_key = Some(key);
        self.selection_info = self
            .editor
            .content
            .selection_counts()
            .map(|(chars, lines)| {
                if lines > 1 {
                    t!("status.selection_lines", lines = lines, chars = chars).to_string()
                } else {
                    t!("status.selection_chars", chars = chars).to_string()
                }
            });
    }

    /// Shows a message in the status bar (in red for an error).
    fn set_status(&mut self, message: impl ToString, is_error: bool) {
        self.status_message = message.to_string();
        self.status_is_error = is_error;
        self.server_offer = None;
    }

    /// After a file opens: offers to install the language server it could
    /// use, if there is one to install and none is there yet.
    fn offer_language_server(&mut self, path: &Path) {
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            return;
        };
        self.server_offer = crate::lsp_install::offer_for(&extension.to_lowercase())
            .filter(|info| !self.servers.installing.contains(info.binary));
    }

    /// Whether a panel or dialog owns the keyboard, or there is nothing to
    /// edit, so editing commands (Tab, line operations) must leave the buffer
    /// alone. A hidden editor (Preview mode) counts: the user can't see what
    /// they would change.
    fn editing_blocked(&self) -> bool {
        self.current_file.is_none()
            || self.show_preferences
            || !matches!(self.overlay, ActiveOverlay::None)
            || (self.view_mode == ViewMode::Preview && self.preview_kind().is_some())
    }

    /// Runs `m` unless a search field has the keyboard (it owns Tab, Ctrl+A…).
    fn check_search_focus_then(&mut self, m: Message) -> Task<Message> {
        if self.show_search && m.is_editor_shortcut() {
            iced::advanced::widget::operate(crate::search::search_field_focused())
                .map(move |focused| Message::ShortcutChecked(Box::new(m.clone()), focused))
        } else {
            self.handle(m)
        }
    }

    /// One level of indentation, per the tab settings.
    fn indent_unit(&self) -> String {
        if self.config.use_spaces {
            " ".repeat(self.config.tab_width)
        } else {
            "\t".to_string()
        }
    }

    /// What the Tab key types: spaces up to the next tab stop, or a tab.
    fn tab_text(&self) -> String {
        if !self.config.use_spaces {
            return "\t".to_string();
        }
        let (line, col) = self.editor.content.cursor_position();
        let before = self
            .editor
            .content
            .line(line)
            .map(|l| l[..floor_boundary(&l, col)].to_string())
            .unwrap_or_default();
        " ".repeat(text_ops::spaces_to_tab_stop(&before, self.config.tab_width))
    }

    /// Whether `edit` would change the buffer: Backspace at the very start
    /// or Delete at the very end do nothing, and shouldn't leave an undo step.
    fn edit_changes_text(&self, edit: &text_editor::Edit) -> bool {
        if self.editor.content.selection().is_some() {
            return true;
        }
        let (line, col) = self.editor.content.cursor_position();
        match edit {
            text_editor::Edit::Backspace => line > 0 || col > 0,
            text_editor::Edit::Delete => {
                let last = self.editor.content.line_count().saturating_sub(1);
                line < last
                    || self
                        .editor
                        .content
                        .line(line)
                        .is_some_and(|l| col < l.len())
            }
            _ => true,
        }
    }

    /// An action in the text area: undo grouping, auto-indent, auto-pairing,
    /// then the action itself.
    fn editor_action(&mut self, action: text_editor::Action) -> Task<Message> {
        let is_edit = action.is_edit();

        if is_edit {
            if let text_editor::Action::Edit(edit) = &action {
                if !self.edit_changes_text(edit) {
                    return Task::none();
                }
                match edit {
                    text_editor::Edit::Insert(ch) => {
                        if self.editor.content.selection().is_some() {
                            // Typing over a selection is a step of its own.
                            self.push_undo();
                            self.undo_new_group = ch.is_whitespace();
                        } else if ch.is_whitespace() {
                            // Whitespace ends the current word group; next non-ws starts a new one
                            self.undo_new_group = true;
                        } else if self.undo_new_group {
                            // First non-whitespace of a new group: snapshot now
                            self.push_undo();
                            self.undo_new_group = false;
                        }
                        // else: mid-word, stay in current group
                    }
                    _ => {
                        // Backspace, Delete, Enter, Paste: always their own undo point
                        self.push_undo();
                        self.undo_new_group = true;
                    }
                }
                if let text_editor::Edit::Paste(text) = edit {
                    if text.len() > MAX_PASTE_BYTES {
                        self.undo_stack.pop_back();
                        self.set_status(
                            format!(
                                "{}: {}",
                                t!("status.error"),
                                t!("status.err_paste_too_large")
                            ),
                            true,
                        );
                        return Task::none();
                    }
                }
            }
        } else {
            // Motion action: next edit opens a new group
            self.undo_new_group = true;
            self.pending_closers.clear();
        }

        // Auto-indent: replicate leading whitespace of current line on Enter
        if self.config.auto_indent {
            if let text_editor::Action::Edit(text_editor::Edit::Enter) = &action {
                self.pending_closers.clear();
                let (line_idx, col) = self.editor.content.cursor_position();
                // Only the whitespace before the cursor: pressing Enter
                // inside the indentation must not duplicate all of it.
                let indent: String = self
                    .editor
                    .content
                    .line(line_idx)
                    .map(|l| {
                        l.chars()
                            .take_while(|c| *c == ' ' || *c == '\t')
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let indent = indent[..indent.len().min(col)].to_string();
                self.editor
                    .content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Enter));
                if !indent.is_empty() {
                    self.editor.content.perform(text_editor::Action::Edit(
                        text_editor::Edit::Paste(std::sync::Arc::new(indent)),
                    ));
                }
                self.is_dirty = true;
                return Task::none();
            }
        }

        // Auto-pairing of brackets and quotes
        if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = &action {
            if let Some(command) = self.autopair(*ch, &action) {
                return command;
            }
            self.shift_pending_closers(ch.len_utf8());
        } else if is_edit {
            self.pending_closers.clear();
        }

        self.editor.content.perform(action);
        if is_edit {
            self.is_dirty = true;
        }
        Task::none()
    }

    /// Characters typed on the cursor's line push the auto-inserted closers
    /// after it to the right.
    fn shift_pending_closers(&mut self, bytes: usize) {
        let (line, col) = self.editor.content.cursor_position();
        for (l, c, _) in &mut self.pending_closers {
            if *l == line && *c >= col {
                *c += bytes;
            }
        }
    }

    /// (line count, topmost line an edit may touch), taken before an edit.
    fn edit_probe(&self) -> (usize, usize) {
        let (line, _) = self.editor.content.cursor_position();
        let above = self
            .editor
            .content
            .selection()
            .map_or(0, |s| s.matches('\n').count());
        (self.editor.content.line_count(), line.saturating_sub(above))
    }

    fn fold(&mut self, command: crate::code_editor::FoldCommand) {
        let kind = crate::folding::Kind::of(self.editor.language.as_deref());
        self.editor.content.fold(kind, command);
    }

    /// Moves the current file's bookmarks after an edit that changed the line
    /// count (see `edit_probe`).
    fn shift_bookmarks(&mut self, count_before: usize, top_before: usize) {
        let (Some(path), count) = (&self.current_file, self.editor.content.line_count()) else {
            return;
        };
        if count == count_before {
            return;
        }
        let top = top_before.min(self.editor.content.cursor_position().0);
        let delta = count as isize - count_before as isize;
        self.bookmarks.shift(path, top, delta);
        self.shift_problems(top, delta);
    }

    /// Runs an edit of the buffer and moves the bookmarks along with it.
    fn edited(&mut self, edit: impl FnOnce(&mut Self)) {
        let (count, top) = self.edit_probe();
        edit(self);
        self.shift_bookmarks(count, top);
    }

    fn bookmark_lines(&self) -> &[usize] {
        self.current_file
            .as_deref()
            .map_or(&[], |path| self.bookmarks.lines(path))
    }

    fn bookmark_state(&self) -> crate::menu_bar::BookmarkState {
        let line = self.editor.content.cursor_position().0;
        crate::menu_bar::BookmarkState {
            on_line: self
                .current_file
                .as_deref()
                .is_some_and(|path| self.bookmarks.has(path, line)),
            any: !self.bookmark_lines().is_empty(),
        }
    }

    /// Asks the language server where the name under the cursor is defined.
    fn goto_definition(&mut self) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let (line, col) = self.editor.content.cursor_position();
        let Some(word) = self
            .editor
            .content
            .line(line)
            .and_then(|text| crate::definition::word_at(&text, col).map(str::to_string))
        else {
            return Task::none();
        };
        let language = self
            .editor
            .language
            .clone()
            .or_else(|| path.extension().map(|e| e.to_string_lossy().into_owned()));
        // Without a known language only the pattern search can answer.
        let language = language.unwrap_or_default();
        let text = crate::editor::buffer_text(&self.editor.content);
        // Open files: the unsaved text of the modified ones.
        let open: Vec<_> = self
            .sidebar
            .files()
            .iter()
            .filter(|p| **p != path && !is_untitled(p))
            .map(|p| {
                let text = self
                    .file_cache
                    .get(p)
                    .filter(|c| c.dirty)
                    .and_then(|c| c.content.as_ref())
                    .map(|c| c.text());
                (p.clone(), text)
            })
            .collect();
        self.overlay = ActiveOverlay::References(RefList::searching(word.clone()));
        let name = word.clone();
        Task::perform(
            async move {
                // A language server when there is one, the pattern search
                // otherwise.
                match crate::lsp::definition(&language, &path, &text, line, col).await {
                    crate::lsp::Outcome::Unsupported(_) | crate::lsp::Outcome::NoServer(..) => {
                        patterns(word, path, text, line, col, open).await
                    }
                    // On the definition itself: where it is used.
                    crate::lsp::Outcome::AtDefinition => {
                        match crate::lsp::references(&language, &path, &text, line, col).await {
                            // Not used anywhere: said as such.
                            crate::lsp::Outcome::NotFound => crate::lsp::Outcome::AtDefinition,
                            other => other,
                        }
                    }
                    outcome => outcome,
                }
            },
            move |outcome| Message::DefinitionFound(name.clone(), outcome),
        )
    }

    /// Goes to a definition, opening its file if need be.
    fn jump_to(&mut self, hit: crate::definition::Hit) -> Task<Message> {
        self.remember_place();
        self.go_to(hit)
    }

    /// The cursor's place, to come back to with Back.
    fn remember_place(&mut self) {
        let Some(path) = self.current_file.clone() else {
            return;
        };
        let (line, col) = self.editor.content.cursor_position();
        let place = Place { path, line, col };
        if self.nav_back.last() != Some(&place) {
            self.nav_back.push(place);
            if self.nav_back.len() > MAX_PLACES {
                self.nav_back.remove(0);
            }
        }
        self.nav_forward.clear();
    }

    /// Back (or forward) to the previous place of the history.
    fn navigate(&mut self, forward: bool) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let (line, col) = self.editor.content.cursor_position();
        let here = Place { path, line, col };
        let (from, to) = if forward {
            (&mut self.nav_forward, &mut self.nav_back)
        } else {
            (&mut self.nav_back, &mut self.nav_forward)
        };
        // A place identical to the current one is no move at all.
        let target = loop {
            match from.pop() {
                Some(place) if place == here => continue,
                other => break other,
            }
        };
        let Some(target) = target else {
            self.set_status(t!("status.no_more_places"), false);
            return Task::none();
        };
        to.push(here);
        self.go_to(crate::definition::Hit {
            path: target.path,
            line: target.line,
            col: target.col,
            len: 0,
        })
    }

    /// Puts the cursor at a place, opening its file if need be.
    fn go_to(&mut self, hit: crate::definition::Hit) -> Task<Message> {
        let len = hit.len;
        self.undo_new_group = true;
        if self.current_file.as_ref() == Some(&hit.path) {
            self.editor.content.select_bytes(hit.line, hit.col, len);
            return Task::none();
        }
        self.pending_jump = Some((hit.path.clone(), hit.line, hit.col, len));
        let task = self.activate(hit.path);
        // A tab already in memory is active by now.
        self.apply_pending_jump();
        task
    }

    /// Puts the cursor where `pending_jump` says, once its file is active.
    fn apply_pending_jump(&mut self) {
        if let Some((path, line, col, len)) = self.pending_jump.take() {
            if self.current_file.as_ref() == Some(&path) {
                self.editor.content.select_bytes(line, col, len);
            } else if self.pending_open.as_ref() == Some(&path) {
                self.pending_jump = Some((path, line, col, len));
            }
        }
    }

    /// Moves the cursor to the next (or previous) bookmark of the file.
    fn jump_to_bookmark(&mut self, forward: bool) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let (line, _) = self.editor.content.cursor_position();
        let total = self.editor.content.line_count();
        let target = if forward {
            self.bookmarks.next(&path, line, total)
        } else {
            self.bookmarks.previous(&path, line, total)
        };
        match target {
            Some(target) => {
                self.remember_place();
                crate::search::goto_line_start(&mut self.editor.content, target);
                self.undo_new_group = true;
            }
            None => self.set_status(t!("status.no_bookmarks"), false),
        }
        Task::none()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: crate::editor::buffer_text(&self.editor.content),
            cursor: self.editor.content.cursor_position(),
        }
    }

    fn push_undo(&mut self) {
        let snapshot = self.snapshot();
        self.undo_stack.push_back(snapshot);
        trim_history(&mut self.undo_stack);
        self.redo_stack.clear();
    }

    /// Keeps the undo history of the background tabs within a global budget
    /// (the largest histories go first).
    fn enforce_undo_budget(&mut self) {
        let mut total: usize = self.undo_cache.values().map(history_bytes).sum();
        while total > UNDO_TOTAL_BYTES {
            let Some((largest, size)) = self
                .undo_cache
                .iter()
                .map(|(p, h)| (p.clone(), history_bytes(h)))
                .max_by_key(|(_, size)| *size)
            else {
                break;
            };
            self.undo_cache.remove(&largest);
            total -= size;
        }
    }

    /// Puts the cursor at (`line`, byte column `col`), clamped to the buffer.
    fn restore_cursor(&mut self, line: usize, col: usize) {
        self.editor.content.select_bytes(line, col, 0);
    }

    /// Replaces the buffer with a history snapshot and puts the cursor back.
    fn restore_snapshot(&mut self, snap: Snapshot) -> Task<Message> {
        let (line, col) = snap.cursor;
        // Back to the saved text means nothing left to save.
        self.is_dirty = self.saved_fp != Some(fingerprint(&snap.text));
        self.undo_new_group = true;
        self.replace_buffer_text(&snap.text);
        self.restore_cursor(line, col);
        Task::none()
    }

    /// Applies a whole-buffer text transformation as one undo step and moves
    /// the cursor to (`line`, `col`), or selects `selection`. A no-op
    /// transformation changes nothing.
    fn apply_text_edit(
        &mut self,
        new_text: String,
        line: usize,
        col: usize,
        selection: Option<Span>,
    ) -> Task<Message> {
        let old = crate::editor::buffer_text(&self.editor.content);
        if new_text == old {
            return Task::none();
        }
        // One copy of the buffer serves the undo step, the bookmarks and the
        // minimal replacement.
        self.undo_stack.push_back(Snapshot {
            text: old.clone(),
            cursor: self.editor.content.cursor_position(),
        });
        trim_history(&mut self.undo_stack);
        self.redo_stack.clear();
        self.is_dirty = true;
        self.undo_new_group = true;
        if let Some(path) = &self.current_file {
            self.bookmarks.remap(path, &old, &new_text);
        }
        crate::editor::replace_text(&mut self.editor.content, &old, &new_text);
        match selection {
            Some(span) => {
                let len = self.span_len(span);
                self.editor
                    .content
                    .select_bytes(span.start.0, span.start.1, len);
            }
            None => self.restore_cursor(line, col),
        }
        Task::none()
    }

    /// Like `apply_text_edit` for a line command: the cursor (and selection)
    /// keep their place on the text, `line_shift` lines further down.
    fn apply_line_edit(
        &mut self,
        old: &str,
        new_text: String,
        line_shift: isize,
        selection: Option<Span>,
    ) -> Task<Message> {
        let (line, col) = self.editor.content.cursor_position();
        let new_line = (line as isize + line_shift).max(0) as usize;
        let col = remap_col(nth_line(old, line), nth_line(&new_text, new_line), col);
        let selection = selection.map(|s| remap_span(old, &new_text, s, line_shift));
        self.apply_text_edit(new_text, new_line, col, selection)
    }

    fn replace_buffer_text(&mut self, new_text: &str) {
        if let Some(path) = &self.current_file {
            if !self.bookmarks.lines(path).is_empty() {
                let old = crate::editor::buffer_text(&self.editor.content);
                self.bookmarks.remap(path, &old, new_text);
            }
        }
        crate::editor::set_buffer_text(&mut self.editor.content, new_text);
    }

    /// The bytes covered by `span` in the buffer as it is now.
    fn span_len(&self, span: Span) -> usize {
        let content = &self.editor.content;
        let len_of = |i: usize| content.line(i).map_or(0, |l| l.len());
        if span.start.0 == span.end.0 {
            return span.end.1.saturating_sub(span.start.1);
        }
        let middle: usize = (span.start.0 + 1..span.end.0).map(|i| len_of(i) + 1).sum();
        len_of(span.start.0).saturating_sub(span.start.1) + 1 + middle + span.end.1
    }

    /// The span of a selection running over several lines. (`text_editor`
    /// exposes the selected text and the cursor, not the anchor, so the start
    /// is inferred by checking which end of the selection the cursor is on
    /// against the buffer's actual lines.)
    fn selection_span(&self) -> Option<Span> {
        let selected = self.editor.content.selection()?;
        let n = selected.split('\n').count();
        if n < 2 {
            return None;
        }
        let first = selected.split('\n').next()?;
        let last = selected.rsplit('\n').next()?;
        let (cursor_line, cursor_col) = self.editor.content.cursor_position();
        let fits = |start: usize| -> bool {
            let (Some(a), Some(b)) = (
                self.editor.content.line(start),
                self.editor.content.line(start + n - 1),
            ) else {
                return false;
            };
            a.ends_with(first) && b.starts_with(last)
        };
        // Cursor at the end of the selection (the usual drag direction)...
        if cursor_line + 1 >= n && cursor_col == last.len() && fits(cursor_line + 1 - n) {
            let start_line = cursor_line + 1 - n;
            let start_col = self
                .editor
                .content
                .line(start_line)
                .map_or(0, |l| l.len().saturating_sub(first.len()));
            return Some(Span {
                start: (start_line, start_col),
                end: (cursor_line, cursor_col),
            });
        }
        // ...or at its start.
        fits(cursor_line).then_some(Span {
            start: (cursor_line, cursor_col),
            end: (cursor_line + n - 1, last.len()),
        })
    }

    /// The lines a line command applies to: the selected ones, else the
    /// cursor's.
    fn lines_to_edit(&self, span: Option<Span>) -> (usize, usize) {
        span.map_or_else(
            || {
                let line = self.editor.content.cursor_position().0;
                (line, line)
            },
            Span::lines,
        )
    }

    /// Alt+Up / Alt+Down: the selected lines (or the cursor's) one line up or down.
    fn move_lines(&mut self, up: bool) -> Task<Message> {
        if self.editing_blocked() {
            return Task::none();
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        let span = self.selection_span();
        let (first, last) = self.lines_to_edit(span);
        let (new_text, new_first) = text_ops::move_lines(&text, first, last, up);
        let shift = new_first as isize - first as isize;
        self.apply_line_edit(&text, new_text, shift, span)
    }

    /// Auto-closing of brackets and quotes for the typed character `ch`.
    /// `None` means "insert it normally".
    fn autopair(&mut self, ch: char, action: &text_editor::Action) -> Option<Task<Message>> {
        use text_editor::{Action, Edit};

        let closer = match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        let is_quote = matches!(ch, '"' | '\'' | '`');
        let is_closer = matches!(ch, ')' | ']' | '}');
        let enabled = if is_quote {
            self.config.autocomplete_quotes
        } else {
            self.config.autocomplete_brackets
        };
        if !enabled || !(closer.is_some() || is_quote || is_closer) {
            return None;
        }

        let (line, col) = self.editor.content.cursor_position();
        let (before, after) = self
            .editor
            .content
            .line(line)
            .map(|l| {
                let col = col.min(l.len());
                (
                    l.get(..col).and_then(|s| s.chars().next_back()),
                    l.get(col..).and_then(|s| s.chars().next()),
                )
            })
            .unwrap_or((None, None));
        let selection = self.editor.content.selection();

        // Typing the closing character that was inserted automatically steps
        // over it; one the user typed (or any other) is just inserted.
        let pending = self
            .pending_closers
            .iter()
            .position(|&(l, c, p)| l == line && c == col && p == ch);
        if (is_closer || is_quote) && selection.is_none() && after == Some(ch) {
            if let Some(i) = pending {
                self.pending_closers.remove(i);
                // Replace the character with itself rather than moving the
                // cursor: a motion over a line just typed on would panic.
                self.editor.content.perform(Action::Edit(Edit::Delete));
                self.editor.content.perform(Action::Edit(Edit::Insert(ch)));
                return Some(Task::none());
            }
        }
        if is_closer {
            return None;
        }
        let close = closer.unwrap_or(ch);

        // With a selection, surround it instead of replacing it. (The undo
        // step was opened by `editor_action`.)
        if let Some(selected) = selection {
            self.pending_closers.clear();
            self.editor
                .content
                .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(format!(
                    "{ch}{selected}{close}"
                )))));
            self.is_dirty = true;
            return Some(Task::none());
        }

        // Only pair before whitespace / a closer / line end; quotes also
        // not after a word character (apostrophes: don't, it's), and a
        // single quote not at all in Rust, where it mostly starts a lifetime.
        let room = after.is_none_or(|c| {
            c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':' | '.')
        });
        let word_before = before.is_some_and(|c| c.is_alphanumeric() || c == '\\');
        let rust_lifetime = ch == '\'' && self.editor.language.as_deref() == Some("rs");
        if !room || (is_quote && word_before) || rust_lifetime {
            return None;
        }

        self.editor.content.perform(action.clone());
        self.editor
            .content
            .perform(Action::Edit(Edit::Insert(close)));
        self.is_dirty = true;
        // Back over the closer, which then sits right after the cursor.
        let (line, end) = self.editor.content.cursor_position();
        let col = end - close.len_utf8();
        self.editor.content.select_bytes(line, col, 0);
        self.pending_closers.push((line, col, close));
        Some(Task::none())
    }

    /// Whether the tab for `path` holds unsaved changes.
    fn is_path_dirty(&self, path: &Path) -> bool {
        if self.current_file.as_deref() == Some(path) {
            self.is_dirty
        } else {
            self.file_cache.get(path).is_some_and(|e| e.dirty)
        }
    }

    /// Closes a tab, asking first if that would discard unsaved changes.
    fn request_close(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_path_dirty(&path) {
            self.overlay = ActiveOverlay::ConfirmClose(path);
            Task::none()
        } else {
            self.close_path(path)
        }
    }

    /// The tick after every message: the marks of the current file follow
    /// the edits and the repository.
    fn after_message(
        &mut self,
        edited: bool,
        status_before: Option<Option<crate::git::Status>>,
        git_done: bool,
    ) -> Task<Message> {
        // The conflicts are settled or the operation was given up: the next
        // ones are announced again.
        if self.git.conflict_summary().is_none() {
            self.conflict_toast_closed = false;
        }
        // The dialog of conflicts closes once nothing is left to settle.
        if matches!(self.overlay, ActiveOverlay::Conflicts)
            && self
                .git
                .status
                .as_ref()
                .is_some_and(|s| s.conflicts().next().is_none() && s.operation.is_none())
        {
            self.overlay = ActiveOverlay::None;
        }
        // New commits on the remote (a fetch, here or in a terminal): said once.
        if let (Some(Some(before)), Some(now)) = (&status_before, &self.git.status) {
            if before.branch == now.branch && now.behind > before.behind {
                let upstream = now.upstream.clone().unwrap_or_default();
                let n = now.behind - before.behind;
                let news = t!("git.behind_news", n = n, upstream = upstream.as_str());
                self.set_status(news, false);
            }
        }
        let moved = self.current_file != self.marks_asked;
        let status_changed = status_before.is_some_and(|before| before != self.git.status);
        if moved || status_changed || git_done {
            return self.refresh_marks_base();
        }
        // Small files follow every keystroke, big ones wait for the tick.
        if edited && self.editor.content.line_count() < 3000 {
            self.refresh_marks_now();
        }
        Task::none()
    }

    /// Asks git what HEAD has of the current file.
    fn refresh_marks_base(&mut self) -> Task<Message> {
        self.marks_asked = self.current_file.clone();
        let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) else {
            self.marks = None;
            return Task::none();
        };
        Task::perform(
            async move {
                // Not in a repository: no answer at all.
                let base = match crate::git::repo_root(&path).await {
                    Some(root) => match relative_to(&root, &path) {
                        Some(relative) => Some(crate::git::head_text(&root, &relative).await),
                        None => None,
                    },
                    None => None,
                };
                (path, base)
            },
            |(path, base)| Message::MarksBase(path, base),
        )
    }

    fn marks_base_loaded(&mut self, path: PathBuf, base: Option<Option<String>>) {
        if self.current_file.as_ref() != Some(&path) {
            return;
        }
        match base {
            None => self.marks = None,
            Some(base) => {
                self.marks = Some(MarksState {
                    path,
                    base,
                    marks: Default::default(),
                    paint: Default::default(),
                    rev: (u64::MAX, u64::MAX),
                });
                self.refresh_marks_now();
            }
        }
    }

    /// Compares the buffer with the base again, if the text changed.
    fn refresh_marks_now(&mut self) {
        let rev = self.buffer_stamp();
        let Some(state) = self.marks.as_mut().filter(|m| m.rev != rev) else {
            return;
        };
        if self.current_file.as_ref() != Some(&state.path) {
            return;
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        state.marks = crate::git::marks::compute(state.base.as_deref(), &text);
        state.paint = [
            std::sync::Arc::new(state.marks.added.clone()),
            std::sync::Arc::new(state.marks.modified.clone()),
            std::sync::Arc::new(state.marks.deleted.clone()),
        ];
        state.rev = rev;
    }

    /// Which text the buffer holds: changes with every edit and when the text
    /// is replaced as a whole.
    fn buffer_stamp(&self) -> (u64, u64) {
        (self.editor.content.id(), self.editor.content.revision())
    }

    /// The changed-line marks of the current file, for the editor.
    fn change_paint(&self) -> Option<crate::code_editor::ChangePaint> {
        let p = crate::theme::palette(self.config.dark_mode);
        let revert = self
            .revert
            .as_ref()
            .filter(|(path, _, rev)| {
                self.current_file.as_ref() == Some(path) && *rev == self.buffer_stamp()
            })
            .map(|(_, line, _)| *line);
        let state = self
            .marks
            .as_ref()
            .filter(|m| self.current_file.as_ref() == Some(&m.path))?;
        let m = &state.marks;
        if m.added.is_empty() && m.modified.is_empty() && m.deleted.is_empty() {
            return None;
        }
        let rgb = |r, g, b| iced::Color::from_rgb8(r, g, b);
        Some(crate::code_editor::ChangePaint {
            added: state.paint[0].clone(),
            modified: state.paint[1].clone(),
            deleted: state.paint[2].clone(),
            added_color: rgb(0x4c, 0xaf, 0x50),
            modified_color: rgb(0x4a, 0x9f, 0xe0),
            deleted_color: rgb(0x9a, 0x9f, 0xab),
            revert,
            popup_background: p.elevated,
            popup_border: p.border_strong,
            popup_text: p.text,
        })
    }

    /// Writes the modified files that have a place on disk, a moment after
    /// the last edit (never over a file that changed under us).
    fn auto_save_files(&mut self) -> Task<Message> {
        if !self.config.auto_save
            || self.autosaving
            || self.last_edit.elapsed() < std::time::Duration::from_millis(700)
        {
            return Task::none();
        }
        let mut tasks = Vec::new();
        if let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) {
            if self.is_dirty && !self.external_warned && !self.changed_on_disk(&path) {
                let content = crate::editor::buffer_text(&self.editor.content);
                tasks.push(save_command(
                    Some(path.clone()),
                    save_file(path, content, self.format),
                ));
            }
        }
        // The other tabs hold their text aside.
        for (path, entry) in &self.file_cache {
            if entry.dirty
                && !is_untitled(path)
                && entry.stamp.is_some()
                && DiskStamp::of(path) == entry.stamp
            {
                if let Some(content) = entry.text() {
                    tasks.push(save_command(
                        Some(path.clone()),
                        save_file(path.clone(), content, entry.format),
                    ));
                }
            }
        }
        if tasks.is_empty() {
            return Task::none();
        }
        self.autosaving = true;
        Task::batch(tasks)
    }

    /// Whether the blame column is drawn for the current file.
    fn blame_shown(&self) -> bool {
        self.blame
            .as_ref()
            .is_some_and(|b| self.current_file.as_ref() == Some(&b.path))
    }

    /// The blame column of the current file, for the editor.
    fn blame_annotations(&self) -> Option<crate::code_editor::Annotations> {
        self.blame
            .as_ref()
            .filter(|b| self.current_file.as_ref() == Some(&b.path))
            .map(|b| b.annotations.clone())
    }

    /// "Annotate with Git Blame": asks git who changed each line.
    fn start_blame(&mut self) -> Task<Message> {
        let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) else {
            return Task::none();
        };
        Task::perform(
            async move {
                let root = crate::git::repo_root(&path)
                    .await
                    .ok_or_else(|| t!("git.no_repo").to_string())?;
                let relative =
                    relative_to(&root, &path).ok_or_else(|| t!("git.no_repo").to_string())?;
                let lines = crate::git::history::blame(&root, &relative).await?;
                Ok::<_, String>((path, (root, relative, lines)))
            },
            |result| match result {
                Ok((path, data)) => Message::BlameLoaded(path, Ok(data)),
                Err(error) => Message::BlameLoaded(PathBuf::new(), Err(error)),
            },
        )
    }

    fn blame_loaded(
        &mut self,
        path: PathBuf,
        result: Result<(PathBuf, String, Vec<crate::git::history::BlameLine>), String>,
    ) {
        match result {
            Ok((repo, relative, lines)) => {
                let dark = self.config.dark_mode;
                let p = se_theme::palette(dark);
                // One tint per commit, from its hash.
                let tints: [iced::Color; 6] = [
                    iced::Color::from_rgba8(0x6e, 0xa8, 0xff, 0.16),
                    iced::Color::from_rgba8(0x8f, 0xd1, 0x8f, 0.14),
                    iced::Color::from_rgba8(0xff, 0x9b, 0x6b, 0.16),
                    iced::Color::from_rgba8(0xc6, 0x8a, 0xe8, 0.16),
                    iced::Color::from_rgba8(0xe6, 0xc0, 0x6a, 0.16),
                    iced::Color::from_rgba8(0x5e, 0xcf, 0xd1, 0.16),
                ];
                // What a pointer on a line shows, once for each commit.
                let mut details: Vec<Vec<String>> = Vec::new();
                let mut index_of: std::collections::HashMap<&str, usize> =
                    std::collections::HashMap::new();
                for l in lines.iter().filter(|l| l.committed()) {
                    index_of.entry(l.sha.as_str()).or_insert_with(|| {
                        details.push(commit_card(l));
                        details.len() - 1
                    });
                }
                let notes: Vec<crate::code_editor::Annotation> = lines
                    .iter()
                    .map(|l| {
                        let tint = if l.committed() {
                            let n = l
                                .sha
                                .bytes()
                                .take(4)
                                .fold(0usize, |a, b| a * 31 + b as usize);
                            tints[n % tints.len()]
                        } else {
                            iced::Color::TRANSPARENT
                        };
                        let text = if l.committed() {
                            format!(
                                "{}  {}",
                                crate::git_panel::stash::format_date(l.time),
                                l.author
                            )
                        } else {
                            t!("git.not_committed").to_string()
                        };
                        let details = index_of.get(l.sha.as_str()).copied();
                        crate::code_editor::Annotation {
                            text,
                            tint,
                            details,
                        }
                    })
                    .collect();
                self.blame = Some(BlameState {
                    path,
                    repo,
                    relative,
                    annotations: crate::code_editor::Annotations {
                        lines: std::sync::Arc::new(notes),
                        text: p.muted,
                        details: std::sync::Arc::new(details),
                        link: p.accent,
                        card_text: p.text,
                        card_background: p.elevated,
                        card_border: p.border_strong,
                    },
                    shas: lines.iter().map(|l| l.sha.clone()).collect(),
                });
            }
            Err(error) => self.set_status(error, true),
        }
    }

    /// A click on a line of the blame column shows its commit in the Git panel.
    fn blame_line_clicked(&mut self, line: usize) -> Task<Message> {
        let Some(blame) = &self.blame else {
            return Task::none();
        };
        let Some(sha) = blame.shas.get(line).cloned() else {
            return Task::none();
        };
        if sha.chars().all(|c| c == '0') {
            return Task::none();
        }
        let (repo, relative) = (blame.repo.clone(), blame.relative.clone());
        self.terminal.open = false;
        self.terminal.focused = false;
        self.git.reveal_commit(sha, Some(relative), repo)
    }

    /// Opens a conflicted file for settling by hand.
    fn open_merge_tab(
        &mut self,
        spec: std::sync::Arc<crate::git_panel::MergeSpec>,
    ) -> Task<Message> {
        let spec =
            std::sync::Arc::try_unwrap(spec).unwrap_or_else(|arc| crate::git_panel::MergeSpec {
                repo: arc.repo.clone(),
                path: arc.path.clone(),
                merge: arc.merge.clone(),
            });
        let existing = self
            .merge_tabs
            .iter()
            .find(|(_, t)| t.repo == spec.repo && t.path == spec.path)
            .map(|(p, _)| p.clone());
        let path = existing.unwrap_or_else(|| {
            self.merge_counter += 1;
            PathBuf::from(format!("merge://{}/{}", self.merge_counter, spec.path))
        });
        self.merge_tabs.insert(
            path.clone(),
            crate::git_panel::merge::MergeTab::new(spec.repo, spec.path, spec.merge),
        );
        self.overlay = ActiveOverlay::None;
        self.sidebar.add_file(path.clone());
        self.activate(path)
    }

    /// The diff the clipboard keys act on: the tab in front, or the panel's
    /// once rows were picked there (and the editor has nothing selected).
    fn diff_viewer(&mut self) -> Option<&mut crate::git_panel::Viewer> {
        if self.on_diff_tab() {
            let path = self.current_file.clone()?;
            return self.diff_tabs.get_mut(&path).map(|tab| &mut tab.viewer);
        }
        if self.git.open
            && !self.git.typing
            && self.editor.content.selection().is_none()
            && self.git.active_viewer().selection.is_some()
        {
            return Some(self.git.active_viewer());
        }
        None
    }

    /// The Git panel's diff was clicked last (and the editor has nothing
    /// selected): the keyboard shortcuts of a diff are for it.
    fn panel_diff_focused(&self) -> bool {
        self.git.open
            && !self.git.typing
            && self.editor.content.selection().is_none()
            && self.git.viewer.selection.is_some()
    }

    fn diff_clipboard(&mut self, key: &str) -> Option<Task<Message>> {
        let viewer = self.diff_viewer()?;
        match key.to_ascii_lowercase().as_str() {
            "a" => {
                viewer.select_all();
                Some(Task::none())
            }
            "c" | "x" => viewer.selected_text().map(iced::clipboard::write),
            _ => None,
        }
    }

    fn on_diff_tab(&self) -> bool {
        self.current_file.as_deref().is_some_and(is_diff)
    }

    /// Keeps what a file holds now, before something rewrites it.
    fn remember_file(&mut self, path: PathBuf) {
        if let Ok(text) = file_io::read_text(&path) {
            self.work_undo.push((path, text));
            if self.work_undo.len() > 50 {
                self.work_undo.remove(0);
            }
            self.work_redo.clear();
        }
    }

    /// A rollback or a delete from the Git panel: the file as it was is kept.
    fn remember_before_git(&mut self, msg: &crate::git_panel::GitMsg) {
        use crate::git_panel::GitMsg;
        let viewer = &self.git.viewer;
        let entry = match msg {
            GitMsg::RollbackBlock(_) => viewer.current(),
            GitMsg::Rollback(i) | GitMsg::Delete(i) => viewer.entries.get(*i),
            _ => None,
        };
        let (Some(repo), Some(entry)) = (self.git.repo.as_ref(), entry) else {
            return;
        };
        let path = repo.join(&entry.change.path);
        self.remember_file(path);
    }

    /// Ctrl+Z (or Ctrl+Y) in a diff: puts back the file as it was before the
    /// last rollback or arrow (or again after it).
    fn undo_work(&mut self, undo: bool) -> Task<Message> {
        // Only what happened to the files this diff is about: a tab's own
        // files, or anything in the repository of the panel.
        let (repo, files): (Option<PathBuf>, Option<Vec<PathBuf>>) = match self
            .current_file
            .as_ref()
            .and_then(|p| self.diff_tabs.get(p))
        {
            Some(tab) => (
                Some(tab.repo.clone()),
                Some(
                    tab.viewer
                        .entries
                        .iter()
                        .map(|e| tab.repo.join(&e.change.path))
                        .collect(),
                ),
            ),
            None => (self.git.repo.clone(), None),
        };
        let in_scope = |path: &PathBuf| {
            repo.as_ref().is_some_and(|r| path.starts_with(r))
                && files.as_ref().is_none_or(|f| f.contains(path))
        };
        let stack = if undo {
            &mut self.work_undo
        } else {
            &mut self.work_redo
        };
        let popped = stack
            .iter()
            .rposition(|(path, _)| in_scope(path))
            .map(|i| stack.remove(i));
        let Some((path, text)) = popped else {
            self.set_status(t!("status.nothing_to_undo"), false);
            return Task::none();
        };
        // What the file holds now is what the opposite key gives back.
        let now = file_io::read_text(&path).unwrap_or_default();
        let (other, label) = if undo {
            (&mut self.work_redo, t!("status.diff_undone"))
        } else {
            (&mut self.work_undo, t!("status.diff_redone"))
        };
        other.push((path.clone(), now));
        if let Err(e) = crate::fsutil::write_atomic(&path, text.as_bytes(), false) {
            self.set_status(format!("{}: {e}", t!("status.error")), true);
            return Task::none();
        }
        self.set_status(label, false);
        Task::batch([
            self.refresh_diff_tabs(),
            Task::done(Message::Git(crate::git_panel::GitMsg::Refresh)),
        ])
    }

    fn on_merge_tab(&self) -> bool {
        self.current_file
            .as_ref()
            .is_some_and(|p| self.merge_tabs.contains_key(p))
    }

    /// The buttons of the merge tab in front.
    fn merge_msg(&mut self, msg: crate::git_panel::merge::MergeMsg) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let Some(tab) = self.merge_tabs.get_mut(&path) else {
            return Task::none();
        };
        // Typing in a result: the app's shortcuts stay out of it.
        if matches!(msg, crate::git_panel::merge::MergeMsg::Edit(..)) {
            self.git.typing = true;
        }
        if matches!(msg, crate::git_panel::merge::MergeMsg::Apply) {
            if tab.merge.unresolved() > 0 {
                return Task::none();
            }
            let (repo, relative, text) = (tab.repo.clone(), tab.path.clone(), tab.merge.render());
            return Task::perform(
                async move { crate::git::resolve_with(&repo, &relative, &text).await },
                move |result| Message::MergeApplied(path.clone(), result),
            );
        }
        tab.update(msg)
    }

    /// The pop-up that announces conflicts, with the button that settles
    /// them: shown while there are some, unless closed or something else
    /// is already asking for attention.
    fn conflict_toast(&self, dark: bool) -> Option<Element<'_, Message>> {
        if self.conflict_toast_closed
            || self.show_preferences
            || !matches!(self.overlay, ActiveOverlay::None)
        {
            return None;
        }
        let label = self.git.conflict_summary()?;
        let p = se_theme::palette(dark);
        let card = container(
            row![
                text("⚠").size(15).color(p.accent),
                text(label).size(13),
                button(text(t!("git.resolve_short").to_string()).size(12.5))
                    .padding([4, 12])
                    .on_press(Message::OpenConflicts)
                    .style((se_theme::Primary { dark }).style()),
                button(text("×").size(15))
                    .padding([0, 7])
                    .on_press(Message::CloseConflictToast)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style()
                    ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .style(se_theme::card(dark));
        Some(
            container(iced::widget::opaque(card))
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .padding(iced::Padding {
                    top: 46.0,
                    ..iced::Padding::ZERO
                })
                .into(),
        )
    }

    /// What the status bar offers: settle conflicts first, else a language server.
    fn status_offer(&self) -> Option<(String, String, Message)> {
        let conflicts = self
            .git
            .status
            .as_ref()
            .map_or(0, |s| s.conflicts().count());
        if conflicts > 0 {
            return Some((
                t!("git.conflict_bar").to_string(),
                t!("git.resolve_short").to_string(),
                Message::OpenConflicts,
            ));
        }
        self.server_offer.map(|info| {
            (
                t!("status.server_offer", language = info.label).to_string(),
                t!("prefs.server_install").to_string(),
                Message::InstallServer(info.binary),
            )
        })
    }

    /// The diff of the working tree in front follows the files (the others
    /// look again when they come to the front).
    fn refresh_diff_tabs(&mut self) -> Task<Message> {
        use crate::git::Source;
        let Some(tab) = self
            .current_file
            .as_ref()
            .and_then(|path| self.diff_tabs.get_mut(path))
        else {
            return Task::none();
        };
        let working_tree = tab.viewer.current().is_some_and(|e| {
            matches!(
                e.source,
                Source::Staged | Source::Unstaged | Source::Untracked
            )
        });
        if !working_tree {
            return Task::none();
        }
        let repo = tab.repo.clone();
        tab.viewer.refresh(&repo)
    }

    /// Opens a diff in an editor tab (one with the same title is reused).
    fn open_diff_tab(&mut self, tab: crate::git_panel::DiffTab) -> Task<Message> {
        let existing = self
            .diff_tabs
            .keys()
            .find(|p| display_name(p) == format!("Δ {}", tab.title))
            .cloned();
        let path = existing.unwrap_or_else(|| {
            self.diff_counter += 1;
            PathBuf::from(format!("diff://{}/{}", self.diff_counter, tab.title))
        });
        let mut viewer = crate::git_panel::Viewer::default();
        viewer.entries = tab.entries;
        let load = viewer.open(&tab.repo, tab.start, crate::git_panel::Land::First);
        self.diff_tabs.insert(
            path.clone(),
            DiffTabState {
                repo: tab.repo,
                viewer,
            },
        );
        self.sidebar.add_file(path.clone());
        let show = self.activate(path);
        Task::batch([show, load])
    }

    /// The arrows of the diff tab in front.
    fn diff_nav(&mut self, nav: DiffNav) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let Some(tab) = self.diff_tabs.get_mut(&path) else {
            return Task::none();
        };
        let repo = tab.repo.clone();
        match nav {
            DiffNav::NextChange => tab.viewer.next_change(&repo),
            DiffNav::PrevChange => tab.viewer.prev_change(&repo),
            DiffNav::NextFile => tab.viewer.next_file(&repo, crate::git_panel::Land::First),
            DiffNav::PrevFile => tab.viewer.prev_file(&repo, crate::git_panel::Land::First),
            DiffNav::Pick(pick) => {
                tab.viewer.pick(pick);
                Task::none()
            }
            DiffNav::Scrolled(top, height) => {
                tab.viewer.view_top.set(top as f32);
                tab.viewer.view_height.set(height as f32);
                Task::none()
            }
            DiffNav::CopyPicked => match tab.viewer.selected_text() {
                Some(text) => iced::clipboard::write(text),
                None => Task::none(),
            },
            DiffNav::PickAll => {
                tab.viewer.select_all();
                Task::none()
            }
            DiffNav::ToggleSide => {
                tab.viewer.side_by_side = !tab.viewer.side_by_side;
                tab.viewer.selection = None;
                tab.viewer.scroll_to_change()
            }
            DiffNav::ToggleCompare => {
                let Some(index) = tab.viewer.index else {
                    return Task::none();
                };
                if let Some(entry) = tab.viewer.entries.get_mut(index) {
                    entry.source = match entry.source.clone() {
                        crate::git::Source::Commit(rev) => crate::git::Source::Tree(rev),
                        crate::git::Source::Tree(rev) => crate::git::Source::Commit(rev),
                        other => other,
                    };
                }
                tab.viewer.open(&repo, index, crate::git_panel::Land::First)
            }
            DiffNav::OpenFile => {
                let Some(entry) = tab.viewer.current() else {
                    return Task::none();
                };
                let file = repo.join(&entry.change.path);
                if !file.is_file() {
                    self.set_status(t!("git.file_gone"), true);
                    return Task::none();
                }
                self.activate(file)
            }
            DiffNav::StageBlock(_) | DiffNav::StageSelection => {
                let block = match nav {
                    DiffNav::StageBlock(b) => Some(b),
                    _ => None,
                };
                let Some((patch, reverse)) = tab.viewer.index_patch(block) else {
                    return Task::none();
                };
                Task::perform(
                    async move { crate::git::apply_patch(&repo, &patch, reverse).await },
                    Message::DiffApplied,
                )
            }
            DiffNav::ToggleBlanks => {
                tab.viewer.ignore_blanks = !tab.viewer.ignore_blanks;
                tab.viewer.reopen(&repo)
            }
            DiffNav::CycleContext => {
                tab.viewer.cycle_context();
                tab.viewer.reopen(&repo)
            }
            DiffNav::Apply(block) => {
                let (Some(entry), Some(diff)) = (
                    tab.viewer.current().cloned(),
                    tab.viewer.file_diff().cloned(),
                ) else {
                    return Task::none();
                };
                self.remember_file(repo.join(&entry.change.path));
                Task::perform(
                    async move {
                        crate::git::rollback_block(&repo, &entry.change.path, &diff, block).await
                    },
                    Message::DiffApplied,
                )
            }
        }
    }

    /// Opens a terminal in the project of the current file (or the home folder).
    fn new_terminal(&mut self) {
        let cwd = self
            .current_file
            .as_deref()
            .filter(|p| !is_untitled(p))
            .and_then(crate::definition::project_root)
            .filter(|p| p.is_dir())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        if let Err(error) = self.terminal.add(&cwd, &t!("terminal.local")) {
            self.set_status(format!("{}: {error}", t!("status.error")), true);
        }
    }

    /// Closes every tab but `keep` (none: all of them), asking once first if
    /// some of them hold unsaved changes.
    fn request_close_many(&mut self, keep: Option<PathBuf>) -> Task<Message> {
        let targets: Vec<PathBuf> = self
            .sidebar
            .files()
            .iter()
            .filter(|p| Some(*p) != keep.as_ref())
            .cloned()
            .collect();
        if targets.is_empty() {
            return Task::none();
        }
        // The tab that stays is the one in front.
        let show = match keep {
            Some(path) if self.current_file.as_ref() != Some(&path) => self.activate(path),
            _ => Task::none(),
        };
        let modified = targets.iter().filter(|p| self.is_path_dirty(p)).count();
        if modified > 0 {
            self.overlay = ActiveOverlay::ConfirmCloseMany(targets, modified);
            show
        } else {
            Task::batch([show, self.close_many(targets)])
        }
    }

    fn close_many(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        Task::batch(paths.into_iter().map(|path| self.close_path(path)))
    }

    /// Removes a tab and, if it was the active one, switches to its neighbour.
    fn close_path(&mut self, path: PathBuf) -> Task<Message> {
        let index = self.sidebar.files().iter().position(|p| *p == path);
        if self.preview_tab.as_ref() == Some(&path) {
            self.preview_tab = None;
        }
        // A file's tab can be opened again (Ctrl+Shift+T).
        if index.is_some() && !is_untitled(&path) && !is_virtual(&path) {
            self.closed_tabs.retain(|p| *p != path);
            self.closed_tabs.push(path.clone());
            if self.closed_tabs.len() > 50 {
                self.closed_tabs.remove(0);
            }
        }
        self.file_cache.remove(&path);
        self.undo_cache.remove(&path);
        self.diff_tabs.remove(&path);
        self.merge_tabs.remove(&path);
        self.sidebar.remove_path(&path);
        // An untitled file is gone for good once closed; a saved one keeps
        // its bookmarks for the next time it is opened.
        if is_untitled(&path) {
            self.bookmarks.clear(&path);
        }

        let mut command = Task::none();
        if self.current_file.as_ref() == Some(&path) {
            self.editor = EditorState::new();
            self.reset_view_mode();
            self.current_file = None;
            self.is_dirty = false;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.undo_new_group = true;
            self.search.reset();
            self.format = TextFormat::default();
            self.disk_stamp = None;
            self.saved_fp = None;
            self.set_status(t!("status.ready"), false);
            let files = self.sidebar.files();
            let next = index
                .and_then(|i| files.get(i))
                .or_else(|| files.last())
                .cloned();
            command = match next {
                Some(next) => self.activate(next),
                None => self.ensure_tab(),
            };
        }
        let _ = self.save_session_now();
        command
    }

    /// The editor is never left without a tab: when the last one is closed, a
    /// blank untitled one takes its place (text typed with nowhere to live
    /// would be lost).
    fn ensure_tab(&mut self) -> Task<Message> {
        if self.current_file.is_some() {
            return Task::none();
        }
        if let Some(first) = self.sidebar.files().first().cloned() {
            return self.activate(first);
        }
        self.untitled_counter = 1;
        self.load_buffer(untitled_path(1), CachedBuffer::blank());
        Task::none()
    }

    /// Whether `path` is one of the scratch files.
    fn is_scratch(&self, path: &Path) -> bool {
        crate::scratch::is_scratch(&self.scratch.dir, path)
    }

    /// Writes the scratch file in front if it was typed in: they are saved by
    /// themselves, with no question asked.
    fn flush_scratch(&mut self) {
        let Some(path) = self.current_file.clone().filter(|p| self.is_scratch(p)) else {
            return;
        };
        if !self.is_dirty || self.changed_on_disk(&path) {
            return;
        }
        let content = crate::editor::buffer_text(&self.editor.content);
        let bytes = self.format.encode(&content);
        match crate::fsutil::write_atomic(&path, bytes.as_bytes(), false) {
            Ok(()) => {
                self.is_dirty = false;
                self.saved_fp = Some(fingerprint(&content));
                self.disk_stamp = DiskStamp::of(&path);
                self.external_warned = false;
                self.size_for = None;
            }
            Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
        }
    }

    /// Shows a scratch file in the editor (it has no tab of its own).
    fn open_scratch(&mut self, path: PathBuf) -> Task<Message> {
        if !path.is_file() {
            self.scratch.error = Some(t!("scratch.err_missing").to_string());
            self.scratch.refresh();
            return Task::none();
        }
        self.flush_scratch();
        self.activate(path)
    }

    /// Keeps the scratch files elsewhere: they move along with it.
    fn set_scratch_dir(&mut self, setting: String) -> Task<Message> {
        let setting = setting.trim().to_string();
        self.preferences.scratch_draft = setting.clone();
        let (old, new) = (self.scratch.dir.clone(), crate::scratch::dir_of(&setting));
        if old == new {
            self.config.scratch_dir = setting;
            self.config_dirty = true;
            return Task::none();
        }
        self.flush_scratch();
        // Their buffers are closed first: the files are about to move.
        let open: Vec<PathBuf> = self
            .file_cache
            .keys()
            .chain(self.current_file.as_ref())
            .filter(|p| self.is_scratch(p))
            .cloned()
            .collect();
        let mut tasks: Vec<Task<Message>> = open.into_iter().map(|p| self.close_path(p)).collect();
        match crate::scratch::relocate(&old, &new) {
            Ok(_) => {
                self.config.scratch_dir = setting;
                self.config_dirty = true;
                self.scratch.dir = new;
                self.scratch.error = None;
            }
            Err(e) => {
                self.preferences.scratch_draft = self.config.scratch_dir.clone();
                self.set_status(format!("{}: {e}", t!("status.error")), true);
            }
        }
        self.scratch.refresh();
        tasks.push(self.ensure_tab());
        Task::batch(tasks)
    }

    /// A file of the tree, clicked: it takes the preview tab (the file that
    /// was in it goes, unless it was edited); clicked twice, it stays.
    fn tree_open(&mut self, path: PathBuf) -> Task<Message> {
        self.tree_focus = true;
        self.projects.selected = Some(path.clone());
        let now = std::time::Instant::now();
        let twice = self.tree_click.as_ref().is_some_and(|(p, at)| {
            *p == path && now.duration_since(*at) < std::time::Duration::from_millis(450)
        });
        self.tree_click = Some((path.clone(), now));
        if twice {
            if self.preview_tab.as_ref() == Some(&path) {
                self.preview_tab = None;
            }
            return self.activate(path);
        }
        // Already a tab (the preview, or one that stays): it is shown.
        if self.sidebar.files().contains(&path) {
            return self.activate(path);
        }
        let dirty = self.dirty_files();
        if let Some(old) = self.preview_tab.take() {
            if !dirty.contains(&old) {
                if let Some(index) = self.sidebar.files().iter().position(|p| *p == old) {
                    self.preview_replaces = Some((old, index));
                }
            }
        }
        self.preview_tab = Some(path.clone());
        self.activate(path)
    }

    /// Once the file of the preview is in the editor, the one it replaces
    /// goes and it takes its place; once it is edited, it stays.
    fn settle_preview(&mut self) {
        let Some(preview) = self.preview_tab.clone() else {
            return;
        };
        if self.current_file.as_ref() != Some(&preview) {
            return;
        }
        if self.is_dirty {
            self.preview_tab = None;
        }
        if let Some((old, index)) = self.preview_replaces.take() {
            if old != preview {
                let _ = self.close_path(old);
                let files = self.sidebar.files();
                if let Some(at) = files.iter().position(|p| *p == preview) {
                    let to = index.min(files.len().saturating_sub(1));
                    self.sidebar.move_tab(at, to);
                }
            }
        }
    }

    /// The file in the editor goes first in the order of the ones seen.
    fn track_mru(&mut self) {
        if self.tab_switch.is_some() {
            return;
        }
        if let Some(current) = &self.current_file {
            if self.mru.first() != Some(current) {
                self.mru.retain(|p| p != current);
                self.mru.insert(0, current.clone());
                self.mru.truncate(200);
            }
        }
    }

    /// Ctrl+Tab: the open files in the order they were seen, the one in the
    /// editor first; each press while Ctrl is held goes one further.
    fn switch_tab(&mut self, forward: bool) -> Task<Message> {
        if self.tab_switch.is_none() {
            let open = self.sidebar.files();
            let mut order: Vec<PathBuf> = self
                .mru
                .iter()
                .filter(|p| open.contains(p))
                .cloned()
                .collect();
            let rest: Vec<PathBuf> = open
                .iter()
                .filter(|p| !order.contains(p))
                .cloned()
                .collect();
            order.extend(rest);
            self.tab_switch = Some((order, 0));
        }
        let Some((order, at)) = &mut self.tab_switch else {
            return Task::none();
        };
        if order.len() < 2 {
            return Task::none();
        }
        *at = if forward {
            (*at + 1) % order.len()
        } else {
            (*at + order.len() - 1) % order.len()
        };
        let target = order[*at].clone();
        self.activate(target)
    }

    /// Ctrl+Shift+T: the last tab closed that can still be read.
    fn reopen_tab(&mut self) -> Task<Message> {
        while let Some(path) = self.closed_tabs.pop() {
            if self.sidebar.files().contains(&path) || !path.is_file() {
                continue;
            }
            return self.activate(path);
        }
        Task::none()
    }

    /// Asks Git how the files of the projects and of the tabs stand.
    fn refresh_file_states(&self) -> Task<Message> {
        let mut dirs: Vec<PathBuf> = self.projects.roots.clone();
        for file in self.sidebar.files() {
            if let Some(dir) = file
                .parent()
                .filter(|_| !is_untitled(file) && !is_virtual(file))
            {
                if !dirs.iter().any(|d| dir.starts_with(d)) {
                    dirs.push(dir.to_path_buf());
                }
            }
        }
        if dirs.is_empty() {
            return Task::none();
        }
        Task::perform(crate::git::file_states_of(dirs), Message::FileStates)
    }

    /// Scrolls the tree just enough to show the line picked.
    fn scroll_tree_to_selected(&self) -> Task<Message> {
        let Some(index) = self
            .projects
            .selected
            .as_ref()
            .and_then(|s| self.projects.index_of(s))
        else {
            return Task::none();
        };
        let row = crate::sidebar::TREE_ROW + 1.0;
        let (offset, height) = self.tree_view;
        let top = index as f32 * row;
        let y = if top < offset {
            top
        } else if height > 0.0 && top + row > offset + height {
            top + row - height
        } else {
            return Task::none();
        };
        iced::widget::operation::scroll_to(
            crate::sidebar::tree_id(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    /// Keeps the projects and their open folders in the settings.
    fn save_projects(&mut self) {
        self.config.projects = self.projects.roots.clone();
        self.config.projects_open = self.projects.expanded.iter().cloned().collect();
        self.config_dirty = true;
    }

    /// Parks the active buffer (text, cursor, scroll, language, dirty flag,
    /// undo history) so another file can take the editor.
    fn stash_current(&mut self) {
        self.flush_scratch();
        if let Some(current) = self.current_file.clone() {
            let content = std::mem::take(&mut self.editor.content);
            self.file_cache.insert(
                current.clone(),
                CachedBuffer {
                    content: Some(content),
                    language: self.editor.language.clone(),
                    dirty: self.is_dirty,
                    stamp: self.disk_stamp,
                    format: self.format,
                    saved_fp: self.saved_fp,
                    view_mode: self.view_mode,
                },
            );
            self.undo_cache.insert(
                current,
                (
                    std::mem::take(&mut self.undo_stack),
                    std::mem::take(&mut self.redo_stack),
                ),
            );
            self.enforce_undo_budget();
        }
    }

    /// Puts a buffer in the editor and makes it the current tab.
    fn load_buffer(&mut self, path: PathBuf, buffer: CachedBuffer) {
        let content = buffer.content.unwrap_or_default();
        let first_line = first_line_of(&content);
        self.editor = EditorState {
            content,
            language: None,
        };
        self.editor
            .restore_language(buffer.language, &path, &first_line);
        self.reset_view_mode();
        self.view_mode = buffer.view_mode;
        if !self.is_scratch(&path) {
            self.sidebar.add_file(path.clone());
        }
        let (undo, redo) = self.undo_cache.remove(&path).unwrap_or_default();
        self.undo_stack = undo;
        self.redo_stack = redo;
        self.undo_new_group = true;
        self.pending_closers.clear();
        self.search.reset();
        self.format_error = None;
        self.show_error_panel = false;
        self.format = buffer.format;
        self.disk_stamp = buffer.stamp;
        self.saved_fp = buffer.saved_fp;
        self.external_warned = false;
        self.current_file = Some(path);
        self.is_dirty = buffer.dirty;
        self.text_rev += 1;
    }

    /// Makes a file just read the active tab.
    fn open_loaded(&mut self, loaded: Loaded) {
        let path = loaded.path.clone();
        self.pending_open = None;
        if self.current_file.as_ref() == Some(&path) {
            // Opening the file being edited again: bring it up to date, but
            // never throw away unsaved changes.
            if self.is_dirty {
                self.set_status(t!("status.already_open_modified"), false);
            } else {
                self.apply_reload(loaded);
            }
            return;
        }
        let cached = self.file_cache.remove(&path);
        self.stash_current();
        let mut warn_changed = false;
        let buffer = match cached {
            // A background tab with unsaved edits wins over the disk.
            Some(c) if c.dirty && c.content.is_some() => {
                warn_changed = c.stamp.is_some() && loaded.stamp != c.stamp;
                c
            }
            other => {
                // The tab changed on disk while in the background (a Git
                // rollback, say): what it showed stays one Ctrl+Z away.
                if let Some(old) = other.as_ref().and_then(|c| c.content.as_ref()) {
                    let old_text = crate::editor::buffer_text(old);
                    if old_text != loaded.text {
                        let snapshot = Snapshot {
                            text: old_text,
                            cursor: old.cursor_position(),
                        };
                        let (undo, redo) = self.undo_cache.entry(path.clone()).or_default();
                        undo.push_back(snapshot);
                        trim_history(undo);
                        redo.clear();
                    }
                }
                let view_mode = other.as_ref().map(|c| c.view_mode);
                let language = other.and_then(|c| c.language);
                let mut buffer = CachedBuffer::from_loaded(loaded, language);
                if let Some(mode) = view_mode {
                    buffer.view_mode = mode;
                }
                buffer
            }
        };
        if !self.is_scratch(&path) {
            self.config.add_recent(&path);
            self.config_dirty = true;
        }
        self.load_buffer(path.clone(), buffer);
        if warn_changed {
            self.external_warned = true;
            self.set_status(t!("status.changed_on_disk"), true);
        } else if self.has_very_long_line() {
            self.set_status(t!("status.long_line_hint"), false);
        } else {
            self.set_status(t!("status.file_opened"), false);
            self.offer_language_server(&path);
        }
    }

    /// Whether a line is so long (minified JSON, say) that typing on it is
    /// slow: the status bar then suggests formatting the file.
    fn has_very_long_line(&self) -> bool {
        self.editor
            .language
            .as_deref()
            .is_some_and(crate::formatter::has_formatter)
            && self
                .editor
                .content
                .lines()
                .any(|line| line.len() > LONG_LINE_BYTES)
    }

    /// Replaces the text of the active tab by a newer version from disk,
    /// keeping the undo history (the old text is one Ctrl+Z away), the cursor,
    /// the scroll position, the language and the view mode.
    fn apply_reload(&mut self, loaded: Loaded) {
        if self.current_file.as_ref() != Some(&loaded.path) {
            return;
        }
        if self.is_dirty {
            // Typing started while the file was being read: the disk version
            // is not applied, and saving will ask first.
            self.external_warned = true;
            self.set_status(t!("status.changed_on_disk"), true);
            return;
        }
        let old = crate::editor::buffer_text(&self.editor.content);
        if old != loaded.text {
            let (line, col) = self.editor.content.cursor_position();
            self.push_undo();
            self.undo_new_group = true;
            self.replace_buffer_text(&loaded.text);
            self.restore_cursor(line, col);
            self.text_rev += 1;
        }
        self.saved_fp = Some(fingerprint(&loaded.text));
        self.format = loaded.format;
        self.disk_stamp = loaded.stamp;
        self.external_warned = false;
        self.size_for = None;
        self.set_status(t!("status.file_reloaded"), false);
    }

    /// Switches to a tab: instantly if its buffer is cached, from disk otherwise.
    fn activate(&mut self, path: PathBuf) -> Task<Message> {
        if self.current_file.as_ref() == Some(&path) {
            return Task::none();
        }
        self.pending_open = Some(path.clone());
        let cached = self.file_cache.get(&path);
        if let Some(entry) = cached.filter(|e| e.content.is_some()) {
            // A saved file may have changed since this copy was made.
            let changed =
                !is_untitled(&path) && entry.stamp.is_some() && DiskStamp::of(&path) != entry.stamp;
            if changed && !entry.dirty {
                return Task::perform(read_file(path, false), Message::TabRead);
            }
            let Some(entry) = self.file_cache.remove(&path) else {
                return Task::none();
            };
            self.pending_open = None;
            self.stash_current();
            self.load_buffer(path.clone(), entry);
            if changed {
                // Modified here and there: saving will ask before overwriting.
                self.external_warned = true;
                self.set_status(t!("status.changed_on_disk"), true);
            } else {
                self.set_status(t!("status.file_opened"), false);
                self.offer_language_server(&path);
            }
            Task::none()
        } else if is_untitled(&path) {
            self.pending_open = None;
            self.stash_current();
            self.load_buffer(path, CachedBuffer::blank());
            // A diff that was behind may be out of date.
            self.refresh_diff_tabs()
        } else {
            // TabRead parks the current buffer once the read succeeds.
            Task::perform(read_file(path, false), Message::TabRead)
        }
    }

    /// Whether the file changed on disk since this tab read or wrote it.
    fn changed_on_disk(&self, path: &Path) -> bool {
        !is_untitled(path)
            && self.disk_stamp.is_some()
            && DiskStamp::of(path).is_some_and(|now| Some(now) != self.disk_stamp)
    }

    /// Notices edits made to the current file by other programs: a clean
    /// buffer is reloaded, a modified one is warned about (saving then asks
    /// before overwriting).
    fn check_external_change(&mut self) -> Task<Message> {
        let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) else {
            return Task::none();
        };
        let Some(now) = DiskStamp::of(&path) else {
            return Task::none();
        };
        if Some(now) == self.disk_stamp {
            return Task::none();
        }
        if self.is_dirty {
            if !self.external_warned {
                self.external_warned = true;
                self.set_status(t!("status.changed_on_disk"), true);
            }
            return Task::none();
        }
        // The new state becomes the baseline only once it has been applied.
        Task::perform(read_file(path, false), Message::FileReloaded)
    }

    fn refresh_file_size(&mut self) {
        if self.size_for != self.current_file {
            self.size_for = self.current_file.clone();
            self.file_size = self
                .current_file
                .as_ref()
                .filter(|p| !is_untitled(p))
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
        }
    }

    /// The preview offered by the current buffer, if any.
    fn preview_kind(&self) -> Option<PreviewKind> {
        preview::kind_for(self.editor.language.as_deref())
    }

    /// Back to the plain editor: called whenever the buffer is replaced by a
    /// different file, since folded paths and the mode belong to one document.
    fn reset_view_mode(&mut self) {
        self.view_mode = ViewMode::Raw;
        self.json_collapsed = Default::default();
        self.preview_rev += 1;
        self.preview = PreviewDoc::None;
    }

    /// Reparses the buffer for the preview pane, but only when it is visible
    /// and its text may have changed since the last parse.
    fn refresh_preview(&mut self) {
        let Some(kind) = self.preview_kind() else {
            self.preview = PreviewDoc::None;
            return;
        };
        if self.view_mode == ViewMode::Raw {
            return;
        }
        if !matches!(self.preview, PreviewDoc::None) && self.preview_text_rev == self.text_rev {
            return;
        }

        let src = crate::editor::buffer_text(&self.editor.content);
        self.preview = match kind {
            PreviewKind::Markdown => {
                PreviewDoc::Markdown(std::sync::Arc::new(crate::preview::markdown::parse(&src)))
            }
            PreviewKind::Json => match crate::preview::json::parse(&src) {
                Ok(value) => PreviewDoc::Json(std::sync::Arc::new(value)),
                Err(e) => PreviewDoc::JsonError(e),
            },
        };
        self.preview_text_rev = self.text_rev;
        self.preview_rev += 1;
    }

    /// Shows a formatter's failure: the whole output in the panel, a short
    /// note in the status bar.
    fn show_format_error(&mut self, error: String) {
        let short = crate::formatter::short_error(&error);
        self.format_error = Some(error);
        self.show_error_panel = true;
        self.set_status(format!("{}: {short}", t!("status.format_error")), true);
    }

    /// The file the formatter should believe it is formatting, so it finds
    /// the project's configuration next to it. `None` for an unsaved buffer.
    fn format_target(&self) -> Option<PathBuf> {
        self.current_file.clone().filter(|p| !is_untitled(p))
    }

    /// Opens the "Save as" dialog; the chosen path comes back as `SaveAsChosen`.
    fn start_save_as(&self, origin: Option<PathBuf>) -> Task<Message> {
        let suggestion = origin.clone().filter(|p| !is_untitled(p));
        let name = origin.as_deref().map(file_label);
        Task::perform(pick_save_path(suggestion, name), move |result| {
            Message::SaveAsChosen(origin.clone(), result)
        })
    }
}

/// The first line of the buffer (capped), to recognize a `#!` script.
fn first_line_of(content: &crate::code_editor::Content) -> String {
    content
        .line(0)
        .map(|l| l.chars().take(200).collect())
        .unwrap_or_default()
}

/// The largest char boundary of `s` at or before `index`.
fn floor_boundary(s: &str, index: usize) -> usize {
    let mut index = index.min(s.len());
    while !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// A selection over several lines: (line, byte column) of both ends.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: (usize, usize),
    end: (usize, usize),
}

impl Span {
    /// The lines a line command touches: a selection ending at the very
    /// start of a line doesn't include it.
    fn lines(self) -> (usize, usize) {
        let last = if self.end.1 == 0 && self.end.0 > self.start.0 {
            self.end.0 - 1
        } else {
            self.end.0
        };
        (self.start.0, last)
    }
}

fn nth_line(text: &str, index: usize) -> &str {
    text.split('\n').nth(index).unwrap_or("")
}

/// A column of `old_line` as it reads in `new_line` after an edit that
/// changed the line's length at its start.
fn remap_col(old_line: &str, new_line: &str, col: usize) -> usize {
    if col == 0 {
        return 0;
    }
    let delta = new_line.len() as isize - old_line.len() as isize;
    let col = (col as isize + delta).clamp(0, new_line.len() as isize) as usize;
    floor_boundary(new_line, col)
}

fn remap_span(old: &str, new: &str, span: Span, line_shift: isize) -> Span {
    let map = |(line, col): (usize, usize)| {
        let new_line = (line as isize + line_shift).max(0) as usize;
        (
            new_line,
            remap_col(nth_line(old, line), nth_line(new, new_line), col),
        )
    };
    Span {
        start: map(span.start),
        end: map(span.end),
    }
}

// ─── Editor pane ────────────────────────────────────────────────────────────

impl SimpleEditApp {
    /// The editor pane: the text editor alone, the preview, or both side by
    /// side (the mode switch lives in the panel's toolbar).
    fn editor_pane(
        &self,
        kind: Option<PreviewKind>,
        mode: ViewMode,
        dark: bool,
    ) -> Element<'_, Message> {
        if let Some(tab) = self
            .current_file
            .as_ref()
            .and_then(|p| self.merge_tabs.get(p))
        {
            return tab.view(dark);
        }
        if let Some(tab) = self
            .current_file
            .as_ref()
            .and_then(|p| self.diff_tabs.get(p))
        {
            return crate::git_panel::diff_view::view(
                &tab.viewer,
                &crate::git_panel::diff_view::Nav::tab(),
                dark,
                t!("git.no_diff").to_string(),
            );
        }
        let search = self.show_search.then_some(&self.search);
        let marks = self.bookmark_lines();
        // The context menu's actions (select/cut/copy/paste/format) all act on
        // the raw buffer: it wraps the editor only — not the preview beside
        // it, and not the preview alone, where it would act on text the user
        // can't see.
        let editor = || {
            self.with_context_menu(
                self.editor.view(
                    &self.config,
                    search,
                    marks,
                    self.blame_annotations(),
                    self.change_paint(),
                    self.problem_paint(),
                    self.completion_hook(),
                ),
                dark,
            )
        };
        if kind.is_none() {
            return editor();
        }

        match mode {
            ViewMode::Raw => editor(),
            ViewMode::Preview => self.preview_view(dark),
            ViewMode::Split => row![
                container(editor()).width(Length::FillPortion(54)),
                container(Space::new().width(1))
                    .height(Length::Fill)
                    .style(se_theme::rule(dark)),
                container(self.preview_view(dark)).width(Length::FillPortion(46)),
            ]
            .height(Length::Fill)
            .into(),
        }
    }

    /// The 40px bar above the editor: file name and folder on the left, the
    /// Raw/Preview/Split switch (Markdown, JSON) on the right.
    fn toolbar_view(
        &self,
        kind: Option<PreviewKind>,
        mode: ViewMode,
        dark: bool,
    ) -> Element<'_, Message> {
        let name = self
            .current_file
            .as_deref()
            .map(file_label)
            .unwrap_or_else(|| t!("app.untitled").to_string());
        let dir = self
            .current_file
            .as_deref()
            .filter(|p| !is_untitled(p))
            .and_then(Path::parent)
            .map(short_dir)
            .unwrap_or_default();

        let on_diff = self.current_file.as_deref().is_some_and(is_virtual);
        let right: Element<Message> = match kind {
            _ if on_diff => Space::new().into(),
            Some(kind) => crate::preview::segmented(kind, mode, dark),
            None => Space::new().into(),
        };

        row![
            text(name)
                .size(13)
                .font(se_theme::UI_SEMIBOLD)
                .shaping(iced::widget::text::Shaping::Advanced),
            text(dir)
                .size(12)
                .color(se_theme::muted_text(dark))
                .shaping(iced::widget::text::Shaping::Advanced),
            Space::new().width(Length::Fill),
            right,
        ]
        .spacing(10)
        .padding([0, 14])
        .height(40)
        .align_y(Alignment::Center)
        .into()
    }

    /// Wraps the editor with the right-click menu.
    fn with_context_menu<'a>(
        &self,
        editor_widget: Element<'a, Message>,
        dark: bool,
    ) -> Element<'a, Message> {
        let lbl_select_all = t!("ctx.select_all").to_string();
        let lbl_cut = t!("ctx.cut").to_string();
        let lbl_copy = t!("ctx.copy").to_string();
        let lbl_paste = t!("ctx.paste").to_string();
        let lbl_delete = t!("ctx.delete").to_string();
        let lbl_split_right = t!("menu.view_split_right").to_string();
        let lbl_split_down = t!("menu.view_split_down").to_string();
        let lbl_split_close = t!("menu.view_split_close").to_string();
        let is_split = self.split.is_some();
        let lbl_lint_fix = t!("menu.lint_fix").to_string();
        let lbl_lint_now = t!("menu.lint_check").to_string();
        let has_problems = !self.lint.diagnostics.is_empty();
        let lint_off = self.lint_group_off();
        let lbl_format_sel = t!("ctx.format_selection").to_string();
        let lbl_format_all = t!("ctx.format_all").to_string();
        let has_fmt_ctx = self
            .editor
            .language
            .as_deref()
            .map(crate::formatter::has_formatter)
            .unwrap_or(false);
        let has_sel_ctx = self.selection_info.is_some();
        let sh_select_all = self.config.shortcuts.select_all.display();
        let sh_format_all = self.config.shortcuts.format_code.display();
        // Cut/Copy/Paste are the platform's own keys, not configurable.
        let clip = |letter: &str| {
            if cfg!(target_os = "macos") {
                format!("⌘{letter}")
            } else {
                format!("Ctrl+{letter}")
            }
        };
        let (sh_cut, sh_copy, sh_paste) = (clip("X"), clip("C"), clip("V"));
        let bookmark_state = self.bookmark_state();
        let git_path = self.current_file.clone().filter(|p| git_menu_applies(p));
        let lbl_git = t!("sidebar.git").to_string();
        let sh_bookmark = self.config.shortcuts.toggle_bookmark.display();

        let minimap_width = if self.config.show_minimap {
            crate::code_editor::MINIMAP_WIDTH
        } else {
            0.0
        };
        let gutter_menu = self.gutter_click;
        let blame_on = self.blame_shown();
        // A line of the blame that a commit made: its previous version is there.
        let previous_ok = self.gutter_line.is_some_and(|line| {
            self.blame
                .as_ref()
                .and_then(|b| b.shas.get(line))
                .is_some_and(|sha| !sha.chars().all(|c| c == '0'))
        });
        let lbl_previous = t!("git.blame_previous").to_string();
        let lbl_blame = t!("git.blame").to_string();
        let lbl_blame_close = t!("git.blame_close").to_string();
        let menu = crate::pointer_area::context_menu(editor_widget, move || {
            use crate::menu_bar::{disabled_item, item, separator};
            if gutter_menu {
                let entry = if blame_on {
                    item(lbl_blame_close.clone(), None, dark, Message::CloseBlame)
                } else {
                    item(lbl_blame.clone(), None, dark, Message::BlameFile)
                };
                let mut entries = column![entry]
                    .spacing(1)
                    .padding(6)
                    .width(Length::Fixed(264.0));
                if blame_on {
                    entries = entries.push(if previous_ok {
                        item(lbl_previous.clone(), None, dark, Message::BlamePrevious)
                    } else {
                        disabled_item(lbl_previous.clone(), dark)
                    });
                }
                return container(entries).style(crate::theme::card(dark)).into();
            }
            let fmt_sel_el: Element<'static, Message> = if has_fmt_ctx && has_sel_ctx {
                item(lbl_format_sel.clone(), None, dark, Message::FormatSelection)
            } else {
                disabled_item(lbl_format_sel.clone(), dark)
            };
            let fmt_all_el: Element<'static, Message> = if has_fmt_ctx {
                item(
                    lbl_format_all.clone(),
                    Some(sh_format_all.clone()),
                    dark,
                    Message::FormatFile,
                )
            } else {
                disabled_item(lbl_format_all.clone(), dark)
            };
            let ctx_items: Vec<Element<'static, Message>> = vec![
                item(
                    lbl_cut.clone(),
                    Some(sh_cut.clone()),
                    dark,
                    Message::ContextCut,
                ),
                item(
                    lbl_copy.clone(),
                    Some(sh_copy.clone()),
                    dark,
                    Message::ContextCopy,
                ),
                item(
                    lbl_paste.clone(),
                    Some(sh_paste.clone()),
                    dark,
                    Message::ContextPaste,
                ),
                item(lbl_delete.clone(), None, dark, Message::ContextDelete),
                item(
                    lbl_select_all.clone(),
                    Some(sh_select_all.clone()),
                    dark,
                    Message::SelectAll,
                ),
                separator(dark),
                fmt_sel_el,
                fmt_all_el,
                separator(dark),
            ];
            let mut ctx_items = ctx_items;
            ctx_items.extend(crate::menu_bar::bookmark_items(
                bookmark_state,
                dark,
                Some(sh_bookmark.clone()),
            ));
            ctx_items.push(separator(dark));
            ctx_items.push(match &git_path {
                Some(path) => item(
                    lbl_git.clone(),
                    None,
                    dark,
                    Message::GitFileMenu(path.clone()),
                ),
                None => disabled_item(lbl_git.clone(), dark),
            });
            ctx_items.push(separator(dark));
            if let Some(group) = lint_off {
                ctx_items.push(item(
                    t!("lint.turn_on", name = group.name).to_string(),
                    None,
                    dark,
                    Message::Lint(crate::lint::Setting::Language(group.id, true)),
                ));
            } else {
                ctx_items.push(item(lbl_lint_now.clone(), None, dark, Message::LintNow));
            }
            ctx_items.push(if has_problems {
                item(lbl_lint_fix.clone(), None, dark, Message::LintFixAll)
            } else {
                disabled_item(lbl_lint_fix.clone(), dark)
            });
            ctx_items.push(separator(dark));
            ctx_items.push(item(
                lbl_split_right.clone(),
                None,
                dark,
                Message::Split(true),
            ));
            ctx_items.push(item(
                lbl_split_down.clone(),
                None,
                dark,
                Message::Split(false),
            ));
            if is_split {
                ctx_items.push(item(
                    lbl_split_close.clone(),
                    None,
                    dark,
                    Message::CloseSplit,
                ));
            }
            container(
                column(ctx_items)
                    .spacing(1)
                    .padding(6)
                    .width(Length::Fixed(264.0)),
            )
            .style(crate::theme::card(dark))
            .into()
        });
        crate::floating::RightClick::new(menu, minimap_width, Message::EditorRightClick).into()
    }

    /// Open files with unsaved changes (the dot in the sidebar).
    fn dirty_files(&self) -> HashSet<PathBuf> {
        let mut dirty: HashSet<PathBuf> = self
            .file_cache
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(path, _)| path.clone())
            .collect();
        if let (true, Some(current)) = (self.is_dirty, &self.current_file) {
            dirty.insert(current.clone());
        }
        dirty
    }

    /// The rendered document, or the parser error when the JSON is invalid.
    fn preview_view(&self, dark: bool) -> Element<'_, Message> {
        match &self.preview {
            // `lazy` keeps the built widget tree until the revision changes:
            // rebuilding up to 20 000 JSON rows on every frame is wasteful.
            PreviewDoc::Markdown(blocks) => {
                let blocks = blocks.clone();
                // The wrapper gives `lazy` (which reports a shrink size) the
                // full pane.
                container(iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::markdown::view(&blocks, dark)
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            PreviewDoc::Json(value) => {
                let (value, collapsed) = (value.clone(), self.json_collapsed.clone());
                container(iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::json::view(&value, &collapsed, dark)
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            PreviewDoc::JsonError(err) => container(
                column![
                    text(t!("preview.json_error").to_string())
                        .size(13)
                        .color(se_theme::error_color(dark)),
                    text(err.clone())
                        .size(12)
                        .font(se_theme::MONO)
                        .color(se_theme::muted_text(dark)),
                ]
                .spacing(6),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([16, 20])
            .into(),
            PreviewDoc::None => container(Space::new().height(0))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        }
    }
}

#[path = "app/file_io.rs"]
mod file_io;
use file_io::*;
#[path = "app/bookmark_panel.rs"]
mod bookmark_panel;
#[path = "app/completion.rs"]
mod completion;
#[path = "app/everywhere.rs"]
mod everywhere;
pub(crate) use completion::Floating;
pub(crate) use everywhere::Commit;
#[path = "app/finder.rs"]
mod finder;
#[path = "app/git_file.rs"]
mod git_file;
#[path = "app/lint.rs"]
mod lint;
#[path = "app/project_ops.rs"]
mod project_ops;
#[path = "app/results.rs"]
mod results;
#[path = "app/split.rs"]
mod split;
#[path = "app/views.rs"]
mod views;
pub(crate) use views::resize_edge;

#[cfg(test)]
#[path = "app/tests.rs"]
mod tests;

/// What a tab is called: the file name, or "Untitled N".
pub fn display_name(path: &Path) -> String {
    if is_diff(path) {
        // diff://3/title
        let rest = path.to_str().unwrap_or("").trim_start_matches("diff://");
        let title = rest.split_once('/').map_or(rest, |(_, t)| t);
        format!("Δ {title}")
    } else if is_merge(path) {
        // merge://3/dir/file.rs
        let rest = path.to_str().unwrap_or("").trim_start_matches("merge://");
        let file = rest.split_once('/').map_or(rest, |(_, f)| f);
        format!("⇄ {}", file.rsplit('/').next().unwrap_or(file))
    } else if path.to_str().is_some_and(|s| s.starts_with("rev://")) {
        // rev://4/file.rs @ 1a2b3c4d: a file as an older commit had it
        let rest = path.to_str().unwrap_or("").trim_start_matches("rev://");
        let title = rest.split_once('/').map_or(rest, |(_, t)| t);
        format!("⟲ {title}")
    } else if is_untitled(path) {
        let n = path
            .to_str()
            .unwrap_or("")
            .trim_start_matches("untitled://");
        format!("{} {}", t!("app.untitled"), n)
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".to_string())
    }
}

fn file_label(path: &Path) -> String {
    display_name(path)
}

/// `display_name` of each path, with the parent folder added to the ones
/// that would otherwise read the same.
pub fn unique_labels(paths: &[PathBuf]) -> Vec<String> {
    let names: Vec<String> = paths.iter().map(|p| display_name(p)).collect();
    paths
        .iter()
        .zip(&names)
        .map(|(path, name)| {
            let twins = names.iter().filter(|n| *n == name).count();
            let parent = path
                .parent()
                .and_then(|d| d.file_name())
                .map(|d| d.to_string_lossy().into_owned());
            match parent {
                Some(parent) if twins > 1 && !is_untitled(path) => format!("{name} — {parent}"),
                _ => name.clone(),
            }
        })
        .collect()
}

/// A small modal asking to confirm a destructive action (or cancel).
fn confirm_card(
    dark: bool,
    title: String,
    message: String,
    confirm_label: String,
    confirm: Message,
) -> Element<'static, Message> {
    confirm_card_with(dark, title, message, None, confirm_label, confirm)
}

/// A confirmation card, with something (a checkbox) between the text and the
/// buttons when `extra` is given.
fn confirm_card_with(
    dark: bool,
    title: String,
    message: String,
    extra: Option<Element<'static, Message>>,
    confirm_label: String,
    confirm: Message,
) -> Element<'static, Message> {
    let p = se_theme::palette(dark);
    container(
        column![
            text(title).size(15).color(p.text),
            text(message)
                .size(12)
                .color(p.muted)
                .shaping(iced::widget::text::Shaping::Advanced),
            extra.unwrap_or_else(|| Space::new().height(0).into()),
            row![
                button(text(t!("confirm.cancel").to_string()).size(13))
                    .padding([7, 16])
                    .on_press(Message::CloseOverlay)
                    .style((se_theme::GhostButton { dark, active: true }).style()),
                Space::new().width(Length::Fill),
                button(text(confirm_label).size(13))
                    .padding([7, 16])
                    .on_press(confirm)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style()
                    ),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(14)
        .padding(24),
    )
    .width(360)
    .style(se_theme::card(dark))
    .into()
}

/// A 1px horizontal hairline.
fn rule<'a>(dark: bool) -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(se_theme::rule(dark))
        .into()
}

/// A hairline between two panels side by side.
fn vrule<'a>(dark: bool) -> Element<'a, Message> {
    container(Space::new().width(1).height(Length::Fill))
        .style(se_theme::rule(dark))
        .into()
}

/// A folder for the toolbar: the home directory shown as `~`, with a
/// trailing slash.
fn short_dir(dir: &Path) -> String {
    let shown = match dirs::home_dir()
        .and_then(|home| dir.strip_prefix(&home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    };
    let shown = if shown.ends_with('/') {
        shown
    } else {
        format!("{shown}/")
    };
    // The toolbar has one line: keep the end of a long path (the folder the
    // file is in matters most).
    let count = shown.chars().count();
    if count > MAX_DIR_CHARS {
        let tail: String = shown.chars().skip(count - (MAX_DIR_CHARS - 1)).collect();
        format!("…{tail}")
    } else {
        shown
    }
}

/// Longest folder path shown in the toolbar, in characters.
const MAX_DIR_CHARS: usize = 56;

/// Where the cursor was: an entry of the Back / Forward history.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    path: PathBuf,
    line: usize,
    col: usize,
}

const MAX_PLACES: usize = 100;

/// The pattern search for a definition, off the UI thread (it may read many
/// files).
async fn patterns(
    word: String,
    path: PathBuf,
    text: String,
    line: usize,
    col: usize,
    open: Vec<crate::definition::Source>,
) -> crate::lsp::Outcome {
    use crate::lsp::Outcome;
    let found = tokio::task::spawn_blocking(move || {
        let current = (path.clone(), Some(text));
        crate::definition::locate(&word, &current, line, col, &open)
    })
    .await
    .ok()
    .flatten();
    found.map_or(Outcome::NotFound, Outcome::Found)
}

fn goto_line_id() -> iced::widget::Id {
    iced::widget::Id::new("goto-line")
}

// ─── Helpers ────────────────────────────────────────────────────────────────

pub fn untitled_path(n: u32) -> PathBuf {
    PathBuf::from(format!("untitled://{}", n))
}

/// The question asked before moving `source` into `dir`.
fn move_message(source: &Path, dir: &Path) -> String {
    let (n, d) = (file_label(source), file_label(dir));
    t!("projects.move_message", name = n.as_str(), dir = d.as_str()).to_string()
}

/// Whether `source` may be moved into the folder `dir`: not into itself,
/// what is inside it, or the folder it already is in.
fn movable(source: &Path, dir: &Path) -> bool {
    !dir.starts_with(source) && source.parent() != Some(dir)
}

/// Opens the system's file manager on a folder, or on the folder of a
/// file.
fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

/// Whether a tab can be shown in a pane of the split: not a diff or a merge.
pub fn splittable(path: &Path) -> bool {
    !is_virtual(path)
}

/// A tab that is not a file: an untitled buffer, or a diff.
/// Whether a Git menu makes sense for this tab: a real file.
pub fn git_menu_applies(path: &Path) -> bool {
    !is_virtual(path) && path.is_absolute()
}

/// What a pointer on a blame line shows of its commit: the hash, who and
/// when, then the message wrapped to a readable width.
fn commit_card(line: &crate::git::history::BlameLine) -> Vec<String> {
    let (name, email) = (line.author.as_str(), line.email.as_str());
    let date = crate::git_panel::stash::format_time(line.time);
    let mut card = vec![
        t!("git.blame_commit", sha = line.sha.as_str()).to_string(),
        t!("git.blame_author", name = name, email = email).to_string(),
        t!("git.blame_date", date = date.as_str()).to_string(),
        String::new(),
    ];
    let message: &str = if line.message.is_empty() {
        &line.summary
    } else {
        &line.message
    };
    for paragraph in message.lines() {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > 72 {
                card.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        card.push(current);
    }
    card
}

/// Bounds of the file list's width.
/// The height the bottom panels start with, and the least they can take.
pub(crate) const PANEL_HEIGHT: f32 = 340.0;
pub(crate) const PANEL_MIN_HEIGHT: f32 = 160.0;
/// Where the list starts: the gap, the bar of panels, and a gap.
const SIDEBAR_LEFT: f32 = ACTIVITY_WIDTH + 1.0;
/// The strip at the right of the list that grabs to resize it, before the
/// hairline.
const SIDEBAR_GRIP: f32 = 4.0;
/// Width of the bar of panels.
pub(crate) const ACTIVITY_WIDTH: f32 = 44.0;
const SIDEBAR_MIN: f32 = 160.0;
const SIDEBAR_MAX: f32 = 700.0;

/// A path for a person: the home folder written `~`.
pub fn display_path(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Whether git tracks a file (it is in a repository and in its index).
pub(crate) async fn is_tracked(path: &Path) -> bool {
    let Some(root) = crate::git::repo_root(path).await else {
        return false;
    };
    let Some(rel) = relative_to(&root, path) else {
        return false;
    };
    crate::git::run(&root, &["ls-files", "--error-unmatch", "--", &rel])
        .await
        .is_ok()
}

/// Deletes a file from the disk and, if `from_git` and git tracks it, from
/// the index too (`git rm`). `true`: it was removed from git as well.
async fn delete_file(path: PathBuf, from_git: bool) -> Result<bool, String> {
    if from_git && is_tracked(&path).await {
        if let Some(root) = crate::git::repo_root(&path).await {
            if let Some(rel) = relative_to(&root, &path) {
                crate::git::run(&root, &["rm", "-f", "--", &rel]).await?;
                return Ok(true);
            }
        }
    }
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(false)
}

pub fn is_untitled(path: &Path) -> bool {
    path.to_str()
        .map(|s| {
            s.starts_with("untitled://")
                || s.starts_with("diff://")
                || s.starts_with("merge://")
                || s.starts_with("rev://")
        })
        .unwrap_or(false)
}

/// `path` relative to `root`, comparing real paths when a link is in the way.
fn relative_to(root: &Path, path: &Path) -> Option<String> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_string_lossy().into_owned());
    }
    let (real, real_root) = (path.canonicalize().ok()?, root.canonicalize().ok()?);
    real.strip_prefix(real_root)
        .ok()
        .map(|rel| rel.to_string_lossy().into_owned())
}

/// A tab that shows a diff (`diff://N/title`).
pub fn is_diff(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("diff://"))
}

/// A tab where a conflict is settled (`merge://N/path`).
pub fn is_merge(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("merge://"))
}

/// A tab that is not a file and is never saved or remembered.
pub fn is_virtual(path: &Path) -> bool {
    is_diff(path) || is_merge(path)
}

/// Releases as a column: version and date, then each section and its changes.
fn changelog_list<'a>(dark: bool, entries: &[crate::changelog::Entry]) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut list = column![]
        .spacing(6)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 0.0,
        });
    for entry in entries {
        list = list.push(Space::new().height(8));
        let head = if entry.date.is_empty() {
            format!("v{}", entry.version)
        } else {
            format!("v{} · {}", entry.version, entry.date)
        };
        list = list.push(text(head).size(15).color(p.text));
        for (title, changes) in &entry.sections {
            list = list.push(text(title.clone()).size(12).color(p.accent));
            for change in changes {
                list = list.push(text(format!("•  {change}")).size(13).color(p.muted));
            }
        }
    }
    list.into()
}
