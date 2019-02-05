
//! Figure out what needs codegening. Some of this is redundant, particularly
//! the evaluation of statics, in fact we should copy *exactly* the output of
//! the host evaluation.
//!
//! This contains code from the relevant parts of `rustc`.
//!
//! We proceed by gathering all possible mono items, then go back over the
//! items, possibly applying transforms specific to device capabilities.

use rustc::hir::def_id::{CrateNum, LOCAL_CRATE, };
use rustc::mir::mono::{CodegenUnit, MonoItem, Linkage, Visibility, };
use rustc::ty::query::Providers;
use rustc::ty::{TyCtxt, Instance, };
use rustc::util::nodemap::{DefIdSet, FxHashSet, };
use rustc_mir::monomorphize::{collector::InliningMap,
                              partitioning::partition,
                              partitioning::PartitioningStrategy,
                              MonoItemExt, };
use rustc_data_structures::fx::{FxHashMap};

use std::sync::{Arc, };

use super::driver_data::DriverData;

use lintrinsics::collector::{collect_items_rec, create_fn_mono_item, };
use lintrinsics::stubbing::Stubber;

pub fn provide(providers: &mut Providers) {
  providers.collect_and_partition_mono_items =
    collect_and_partition_mono_items;
}

fn collect_and_partition_mono_items<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                              cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
{
  DriverData::with(tcx, move |tcx, dd| {
    collect_and_partition_mono_items_(tcx, dd, cnum)
  })
}
fn collect_and_partition_mono_items_<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                           dd: &DriverData<'tcx>,
                                           cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
{
  assert_eq!(cnum, LOCAL_CRATE);

  // normally, we start by collecting crate roots (non-generic items
  // to seed monomorphization).
  // We, however, already know the root: it's the root function passed to
  // the kernel_id intrinsic. Plus, we *only* want to codegen what is
  // actually used in the function.

  let root = Instance::mono(tcx, dd.root().did);
  let mono_root = create_fn_mono_item(root);

  let mut visited: FxHashSet<_> = Default::default();
  let mut inlining_map = Some(InliningMap::new());

  let stubber = Stubber::default();

  {
    collect_items_rec(tcx, &stubber, dd,
                      mono_root,
                      &mut visited,
                      &mut inlining_map);
  }
  let inlining_map = inlining_map.unwrap();

  let strategy = PartitioningStrategy::FixedUnitCount(1);
  let items = visited;
  let mut units = partition(tcx, items.iter().cloned(),
                            strategy, &inlining_map);

  // force the root to have an external linkage:
  for unit in units.iter_mut() {
    for mut item in unit.items_mut().iter_mut() {
      if item.0 == &mono_root {
        (item.1).0 = Linkage::External;
      } else {
        (item.1).0 = Linkage::Internal;
      }
      (item.1).1 = Visibility::Default;
    }
  }

  let units: Vec<Arc<CodegenUnit>> = units
    .into_iter()
    .map(Arc::new)
    .collect();

  let mono_items: DefIdSet = items.iter()
    .filter_map(|mono_item| {
      match *mono_item {
        MonoItem::Fn(ref instance) => Some(instance.def_id()),
        MonoItem::Static(def_id) => Some(def_id),
        _ => None,
      }
    })
    .collect();

  if tcx.sess.opts.debugging_opts.print_mono_items.is_some() {
    let mut item_to_cgus: FxHashMap<_, Vec<_>> = Default::default();

    for cgu in &units {
      for (&mono_item, &linkage) in cgu.items() {
        item_to_cgus.entry(mono_item)
          .or_default()
          .push((cgu.name().clone(), linkage));
      }
    }

    let mut item_keys: Vec<_> = items
      .iter()
      .map(|i| {
        let mut output = i.to_string(tcx);
        output.push_str(" @@");
        let mut empty = Vec::new();
        let cgus = item_to_cgus.get_mut(i).unwrap_or(&mut empty);
        cgus.as_mut_slice().sort_by_key(|&(ref name, _)| name.clone());
        cgus.dedup();
        for &(ref cgu_name, (linkage, _)) in cgus.iter() {
          output.push_str(" ");
          output.push_str(&cgu_name.as_str());

          let linkage_abbrev = match linkage {
            Linkage::External => "External",
            Linkage::AvailableExternally => "Available",
            Linkage::LinkOnceAny => "OnceAny",
            Linkage::LinkOnceODR => "OnceODR",
            Linkage::WeakAny => "WeakAny",
            Linkage::WeakODR => "WeakODR",
            Linkage::Appending => "Appending",
            Linkage::Internal => "Internal",
            Linkage::Private => "Private",
            Linkage::ExternalWeak => "ExternalWeak",
            Linkage::Common => "Common",
          };

          output.push_str("[");
          output.push_str(linkage_abbrev);
          output.push_str("]");
        }
        output
      })
      .collect();

    item_keys.sort();

    for item in item_keys {
      println!("MONO_ITEM {}", item);
    }
  }

  (Arc::new(mono_items), Arc::new(units))
}
