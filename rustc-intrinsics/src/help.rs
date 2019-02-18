
use std::iter::{repeat, };

use crate::rustc::mir::{Constant, Operand, Rvalue, };
use crate::rustc::mir::interpret::{ConstValue, Scalar, Pointer,
                                   ScalarMaybeUndef, AllocId,
                                   Allocation, };
use crate::rustc::mir::{self, };
use crate::rustc::ty::{self, TyCtxt, layout::Size, };
use crate::rustc::ty::{Const, ParamEnv, Tuple, Array, };
use crate::rustc_target::abi::{FieldPlacement, Align, HasDataLayout, };
use crate::syntax_pos::{DUMMY_SP, };

// TODO report a helpful message if a closure is given.

pub fn extract_fn_instance<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                     instance: ty::Instance<'tcx>,
                                     local_ty: ty::Ty<'tcx>)
  -> ty::Instance<'tcx>
{
  let reveal_all = ParamEnv::reveal_all();

  let local_ty = tcx
    .subst_and_normalize_erasing_regions(instance.substs,
                                         reveal_all,
                                         &local_ty);

  let instance = match local_ty.sty {
    ty::Ref(_, &ty::TyS {
      sty: ty::FnDef(def_id, subs),
      ..
    }, ..) |
    ty::FnDef(def_id, subs) => {
      let subs = tcx
        .subst_and_normalize_erasing_regions(instance.substs,
                                             reveal_all,
                                             &subs);
      ty::Instance::resolve(tcx, reveal_all, def_id, subs)
        .expect("must be resolvable")
    },
    _ => {
      unreachable!("unexpected param type: {:?}", local_ty);
    },
  };

  instance
}

pub fn extract_opt_fn_instance<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                         instance: ty::Instance<'tcx>,
                                         local_ty: ty::Ty<'tcx>)
  -> Option<ty::Instance<'tcx>>
{
  let reveal_all = ParamEnv::reveal_all();

  let local_ty = tcx
    .subst_and_normalize_erasing_regions(instance.substs,
                                         reveal_all,
                                         &local_ty);

  if local_ty == tcx.types.unit { return None; }

  let instance = match local_ty.sty {
    ty::Ref(_, reffed, _) if reffed == tcx.types.unit => { return None; },
    ty::Ref(_, &ty::TyS {
      sty: ty::FnDef(def_id, subs),
      ..
    }, ..) |
    ty::FnDef(def_id, subs) => {
      let subs = tcx
        .subst_and_normalize_erasing_regions(instance.substs,
                                             reveal_all,
                                             &subs);
      ty::Instance::resolve(tcx, reveal_all, def_id, subs)
        .expect("must be resolvable")
    },
    _ => {
      unreachable!("unexpected param type: {:?}", local_ty);
    },
  };

  Some(instance)
}

/*macro_rules! mk_alloc_fn {
    ($name:ident, $cv:ident, $ty:ident, ) => {
      fn $name(self, v: impl Into<u128>) -> ConstValue<'tcx> {
        let tcx = self.as_tcx();
        let ty = tcx.types.$ty;
        let env = ParamEnv::reveal_all().and(ty);
        let layout = tcx.layout_of(env)
          .expect("layout failure");
        let size = layout.details.size;
        let align = layout.details.align.pref;

        let mut alloc = Allocation::undef(size, align, ());
        let alloc_id = tcx.alloc_map.lock().reserve();

      }
    };
}*/

pub trait LegionellaTyCtxtHelp<'a, 'tcx>: Copy
  where 'tcx: 'a,
{
  fn as_tcx(self) -> TyCtxt<'a, 'tcx, 'tcx>;

  fn mk_const(self, c: ty::Const<'tcx>) -> &'tcx ty::LazyConst {
    let l = ty::LazyConst::Evaluated(c);
    self.as_tcx().intern_lazy_const(l)
  }
  fn mk_const_op(self,
                 src: mir::SourceInfo,
                 c: ty::Const<'tcx>) -> Operand<'tcx> {
    let v = Constant {
      span: src.span,
      ty: c.ty,
      literal: self.mk_const(c),
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  }

  fn mk_bool_cv(self, v: bool) -> ConstValue<'tcx> {
    let v = Scalar::from_bool(v);
    ConstValue::Scalar(v)
  }
  fn mk_u32_cv(self, v: u32) -> ConstValue<'tcx> {
    let v = Scalar::from_uint(v, Size::from_bytes(4));
    ConstValue::Scalar(v)
  }
  fn mk_u64_cv(self, v: u64) -> ConstValue<'tcx> {
    let v = Scalar::from_uint(v, Size::from_bytes(8));
    ConstValue::Scalar(v)
  }
  fn mk_usize_cv(self, v: impl Into<u128>) -> ConstValue<'tcx> {
    let size = self.as_tcx().data_layout().pointer_size;
    let v = Scalar::from_uint(v, size);
    ConstValue::Scalar(v)
  }
  fn mk_usize_c(self, v: impl Into<u128>) -> &'tcx ty::LazyConst<'tcx> {
    self.mk_const(ty::Const {
      ty: self.as_tcx().types.usize,
      val: self.mk_usize_cv(v),
    })
  }

  fn mk_static_str_operand(self, source_info: mir::SourceInfo,
                           v: &str)
    -> Operand<'tcx>
  {
    let tcx = self.as_tcx();
    let id = tcx.allocate_bytes(v.as_bytes());
    let v = ConstValue::new_slice(Scalar::Ptr(id.into()),
                                  v.len() as u64);
    let v = self.mk_const(Const {
      ty: tcx.mk_static_str(),
      val: v,
    });
    let v = Constant {
      span: source_info.span,
      ty: tcx.mk_static_str(),
      literal: v,
      user_ty: None,
    };
    Operand::Constant(Box::new(v))
  }

  fn mk_u64_operand(self, source_info: mir::SourceInfo,
                    v: u64)
    -> Operand<'tcx>
  {
    let tcx = self.as_tcx();
    let v = self.mk_u64_cv(v);
    let v = self.mk_const(Const {
      ty: tcx.types.u64,
      val: v,
    });
    let v = Constant {
      span: source_info.span,
      ty: tcx.types.u64,
      literal: v,
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  }
}
impl<'a, 'tcx> LegionellaTyCtxtHelp<'a, 'tcx> for TyCtxt<'a, 'tcx, 'tcx>
  where 'tcx: 'a,
{
  fn as_tcx(self) -> TyCtxt<'a, 'tcx, 'tcx> { self }
}

// TODO move the following functions into `LegionellaTyCtxtHelp`.

pub fn build_compiler_opt<'a, 'tcx, F, T>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                          val: Option<T>,
                                          some_val: F)
  -> ConstValue<'tcx>
  where F: FnOnce(TyCtxt<'a, 'tcx, 'tcx>, T) -> ConstValue<'tcx>,
{
  if let Some(val) = val {
    let val = some_val(tcx, val);
    let ptr = match val {
      ConstValue::Scalar(Scalar::Ptr(ptr)) => ptr,
      ConstValue::Scalar(Scalar::Bits { size, .. }) => {
        // create an allocation for this

        let scalar = match val {
          ConstValue::Scalar(s) => s,
          _ => unreachable!(),
        };

        let size = Size::from_bytes(size as _);
        let align = Align::from_bytes(1).unwrap();
        let mut alloc = Allocation::undef(size, align, ());
        let alloc_id = tcx.alloc_map.lock().reserve();

        let ptr = Pointer::from(alloc_id);
        alloc.write_scalar(&tcx, ptr,
                           ScalarMaybeUndef::Scalar(scalar),
                           size)
          .expect("allocation write failed");

        let alloc = tcx.intern_const_alloc(alloc);
        tcx.alloc_map.lock().set_alloc_id_memory(alloc_id, alloc);

        ptr
      },
      val => unimplemented!("scalar type {:?}", val),
    };
    ConstValue::new_slice(Scalar::Ptr(ptr), 1)
  } else {
    let s = Scalar::ptr_null(&tcx);
    ConstValue::new_slice(s, 0)
  }
}

pub fn const_value_rvalue<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                    const_val: ConstValue<'tcx>,
                                    ty: ty::Ty<'tcx>)
  -> Rvalue<'tcx>
  where 'tcx: 'a,
{
  let source_info = mir::SourceInfo {
    span: DUMMY_SP,
    scope: mir::OUTERMOST_SOURCE_SCOPE,
  };

  let constant = tcx.mk_const(Const {
    ty,
    val: const_val,
  });
  let constant = Constant {
    span: source_info.span,
    ty,
    literal: constant,
    user_ty: None,
  };
  let constant = Box::new(constant);
  let constant = Operand::Constant(constant);

  Rvalue::Use(constant)
}

pub fn static_str_const_value<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                        s: &str)
  -> ConstValue<'tcx>
{
  let id = tcx.allocate_bytes(s.as_bytes());
  ConstValue::new_slice(Scalar::Ptr(id.into()),
                        s.len() as u64)
}

pub fn static_tuple_const_value<'a, 'tcx, I>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                             what: &str,
                                             tuple: I,
                                             ty: ty::Ty<'tcx>)
  -> ConstValue<'tcx>
  where I: ExactSizeIterator<Item = ConstValue<'tcx>>,
{
  let (alloc_id, ..) = static_tuple_alloc(tcx, what, tuple, ty);
  let ptr = Pointer::from(alloc_id);
  let scalar = Scalar::Ptr(ptr);
  ConstValue::Scalar(scalar)
}

pub fn static_tuple_alloc<'a, 'tcx, I>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       what: &str,
                                       tuple: I,
                                       ty: ty::Ty<'tcx>)
  -> (AllocId, &'tcx Allocation, Size)
  where I: ExactSizeIterator<Item = ConstValue<'tcx>>,
{
  let env = ParamEnv::reveal_all()
    .and(ty);
  let layout = tcx.layout_of(env)
    .expect("layout failure");
  let size = layout.details.size;
  let align = layout.details.align.pref;

  let data = vec![0; size.bytes() as usize];
  let mut alloc = Allocation::from_bytes(&data, align, ());
  let alloc_id = tcx.alloc_map.lock().reserve();

  let mut tuple = tuple.enumerate();

  write_static_tuple(tcx, what, &mut tuple, alloc_id, &mut alloc,
                     Size::ZERO, ty);

  assert_eq!(tuple.next(), None);

  if what != "kernel_id_for" {
    trace!("final alloc bytes for {}: {:#?}", what, {
      let indexed_bytes: Vec<_> = alloc.bytes.iter()
        .cloned()
        .enumerate()
        .collect();
      indexed_bytes
    });
  }

  let alloc = tcx.intern_const_alloc(alloc);
  tcx.alloc_map.lock().set_alloc_id_memory(alloc_id, alloc);
  (alloc_id, alloc, size)
}
pub fn write_static_tuple<'a, 'tcx, I>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       what: &str,
                                       tuple: &mut I,
                                       alloc_id: AllocId,
                                       alloc: &mut Allocation,
                                       base: Size,
                                       ty: ty::Ty<'tcx>)
  where I: ExactSizeIterator<Item = (usize, ConstValue<'tcx>)>,
{
  let env = ParamEnv::reveal_all()
    .and(ty);
  let layout = tcx.layout_of(env)
    .expect("layout failure");

  let fields = match layout.details.fields {
    FieldPlacement::Arbitrary {
      ref offsets,
      ..
    } => {
      offsets.clone()
    },
    FieldPlacement::Array {
      stride, count,
    } => {
      let offsets: Vec<_> = (0..count)
        .map(|idx| stride * idx )
        .collect();
      offsets
    },
    _ => unimplemented!("layout offsets {:?}", layout),
  };

  let ty_fields: Box<Iterator<Item = ty::Ty<'tcx>>> = match ty.sty {
    Tuple(tuple_fields) => {
      let iter = tuple_fields.iter().cloned();
      assert_eq!(iter.len(), fields.len());
      Box::new(iter) as Box<_>
    },
    Array(element, _count) => {
      Box::new(repeat(element)) as Box<_>
    },
    _ => unimplemented!("non tuple type: {:?}", ty),
  };

  for (mut offset, field_ty) in fields.into_iter().zip(ty_fields) {
    match field_ty.sty {
      Tuple(_) => {
        write_static_tuple(tcx, what, tuple, alloc_id, alloc,
                           base + offset, field_ty);
        continue;
      },
      Array(..) => {
        write_static_tuple(tcx, what, tuple, alloc_id, alloc,
                           base + offset, field_ty);
        continue;
      },
      _ => { },
    }

    let (index, element) = tuple.next()
      .expect("missing tuple field value");

    trace!("write tuple: {}, index {} at offset {}, ty: {:?}",
             what, index, (base + offset).bytes(), field_ty);

    let mut write_scalar = |scalar| {
      let ptr = Pointer::new(alloc_id, base + offset);
      let size = match scalar {
        Scalar::Bits { size, .. } => {
          Size::from_bytes(size as _)
        },
        Scalar::Ptr(_) => {
          tcx.data_layout().pointer_size
        },
      };
      offset += size;

      let scalar = ScalarMaybeUndef::Scalar(scalar);
      alloc.write_scalar(&tcx, ptr, scalar, size)
        .expect("allocation write failed");
    };

    match element {
      ConstValue::Scalar(scalar) => {
        write_scalar(scalar);
      },
      ConstValue::Slice(data, len) => {
        write_scalar(data);
        let len = Scalar::from_uint(len, tcx.data_layout().pointer_size);
        write_scalar(len);
      },
      _ => {
        bug!("unhandled ConstValue: {:?}", element);
      },
    }
  }
}

pub fn mk_static_slice<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, elem: ty::Ty<'tcx>) -> ty::Ty<'tcx> {
  tcx.mk_imm_ref(tcx.types.re_static, tcx.mk_slice(elem))
}