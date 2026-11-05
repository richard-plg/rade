//! A row of chips that drops one first when short of room (R146.3: on a
//! Navigator Worktree row the Review chip goes before the PR chip). Lays
//! its visible children out left to right at their natural widths, 6 px
//! apart; when the width given is less than all of them need, the
//! children with CSS class [`DROP_FIRST`] are hidden (`child_visible`)
//! and the rest keep their natural widths.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk4 as gtk;

/// The class of a child that is dropped first.
pub const DROP_FIRST: &str = "drop-first";
const SPACING: i32 = 6;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DropFirst;

    #[glib::object_subclass]
    impl ObjectSubclass for DropFirst {
        const NAME: &'static str = "RadeDropFirstLayout";
        type Type = super::DropFirst;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for DropFirst {}

    fn kids(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut v = Vec::new();
        let mut c = widget.first_child();
        while let Some(w) = c {
            c = w.next_sibling();
            if w.is_visible() {
                v.push(w);
            }
        }
        v
    }

    fn row(widths: impl Iterator<Item = i32>) -> i32 {
        let (sum, n) = widths.fold((0, 0), |(s, n), w| (s + w, n + 1));
        sum + SPACING * (n - 1).max(0)
    }

    impl LayoutManagerImpl for DropFirst {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            _for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let kids = kids(widget);
            let nat = |w: &gtk::Widget| w.measure(orientation, -1).1;
            if orientation == gtk::Orientation::Horizontal {
                let all = row(kids.iter().map(nat));
                let kept = row(kids
                    .iter()
                    .filter(|w| !w.has_css_class(DROP_FIRST))
                    .map(nat));
                (kept, all, -1, -1)
            } else {
                let min = kids
                    .iter()
                    .map(|w| w.measure(orientation, -1).0)
                    .max()
                    .unwrap_or(0);
                let n = kids.iter().map(nat).max().unwrap_or(0);
                (min, n, -1, -1)
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, _baseline: i32) {
            let kids = kids(widget);
            let nat = |w: &gtk::Widget| w.measure(gtk::Orientation::Horizontal, -1).1;
            let drop = width < row(kids.iter().map(nat));
            let shown: Vec<&gtk::Widget> = kids
                .iter()
                .filter(|w| {
                    let keep = !(drop && w.has_css_class(DROP_FIRST));
                    if w.is_child_visible() != keep {
                        w.set_child_visible(keep);
                    }
                    keep
                })
                .collect();
            // Right-aligned: the chips end at the row's end.
            let used = row(shown.iter().map(|w| nat(w)));
            let mut x = (width - used).max(0);
            for w in shown {
                let cw = nat(w);
                w.size_allocate(&gtk::Allocation::new(x, 0, cw, height), -1);
                x += cw + SPACING;
            }
        }
    }
}

glib::wrapper! {
    pub struct DropFirst(ObjectSubclass<imp::DropFirst>) @extends gtk::LayoutManager;
}

impl DropFirst {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for DropFirst {
    fn default() -> Self {
        Self::new()
    }
}
