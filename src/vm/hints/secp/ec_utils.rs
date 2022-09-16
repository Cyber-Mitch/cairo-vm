use crate::bigint;
use crate::math_utils::{ec_double_slope, line_slope};
use crate::serde::deserialize_program::ApTracking;
use crate::types::exec_scope::ExecutionScopesProxy;
use crate::vm::errors::vm_errors::VirtualMachineError;
use crate::vm::hints::hint_utils::{
    get_integer_from_var_name, get_relocatable_from_var_name, insert_value_into_ap,
};
use crate::vm::hints::secp::secp_utils::{pack, pack_from_relocatable, SECP_P};
use crate::vm::vm_core::VMProxy;
use num_bigint::BigInt;
use num_integer::Integer;
use std::collections::HashMap;
use std::ops::BitAnd;

/*
Implements hint:
%{
    from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack

    y = pack(ids.point.y, PRIME) % SECP_P
    # The modulo operation in python always returns a nonnegative number.
    value = (-y) % SECP_P
%}
*/
pub fn ec_negate(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //ids.point
    let point_y = get_relocatable_from_var_name("point", ids, vm_proxy, hint_ap_tracking)? + 3;
    let y = pack_from_relocatable(point_y, vm_proxy)?;
    let value = (-y).mod_floor(&SECP_P);
    exec_scopes_proxy.insert_value("value", value);
    Ok(())
}

/*
Implements hint:
%{
    from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack
    from starkware.python.math_utils import ec_double_slope

    # Compute the slope.
    x = pack(ids.point.x, PRIME)
    y = pack(ids.point.y, PRIME)
    value = slope = ec_double_slope(point=(x, y), alpha=0, p=SECP_P)
%}
*/
pub fn compute_doubling_slope(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //ids.point
    let point_reloc = get_relocatable_from_var_name("point", ids, vm_proxy, hint_ap_tracking)?;

    let (x_d0, x_d1, x_d2, y_d0, y_d1, y_d2) = (
        vm_proxy.memory.borrow().get_integer(&point_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 2))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 3))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 4))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 5))?,
    );

    let value = ec_double_slope(
        (
            pack(x_d0, x_d1, x_d2, vm_proxy.prime),
            pack(y_d0, y_d1, y_d2, vm_proxy.prime),
        ),
        &bigint!(0),
        &SECP_P,
    );
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("slope", value);
    Ok(())
}

/*
Implements hint:
%{
    from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack
    from starkware.python.math_utils import line_slope

    # Compute the slope.
    x0 = pack(ids.point0.x, PRIME)
    y0 = pack(ids.point0.y, PRIME)
    x1 = pack(ids.point1.x, PRIME)
    y1 = pack(ids.point1.y, PRIME)
    value = slope = line_slope(point1=(x0, y0), point2=(x1, y1), p=SECP_P)
%}
*/
pub fn compute_slope(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //ids.point0
    let point0_reloc = get_relocatable_from_var_name("point0", ids, vm_proxy, hint_ap_tracking)?;

    let (point0_x_d0, point0_x_d1, point0_x_d2, point0_y_d0, point0_y_d1, point0_y_d2) = (
        vm_proxy.memory.borrow().get_integer(&point0_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 2))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 3))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 4))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 5))?,
    );

    //ids.point1
    let point1_reloc = get_relocatable_from_var_name("point1", ids, vm_proxy, hint_ap_tracking)?;

    let (point1_x_d0, point1_x_d1, point1_x_d2, point1_y_d0, point1_y_d1, point1_y_d2) = (
        vm_proxy.memory.borrow().get_integer(&point1_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 2))?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 3))?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 4))?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 5))?,
    );

    let value = line_slope(
        &(
            pack(point0_x_d0, point0_x_d1, point0_x_d2, vm_proxy.prime),
            pack(point0_y_d0, point0_y_d1, point0_y_d2, vm_proxy.prime),
        ),
        &(
            pack(point1_x_d0, point1_x_d1, point1_x_d2, vm_proxy.prime),
            pack(point1_y_d0, point1_y_d1, point1_y_d2, vm_proxy.prime),
        ),
        &SECP_P,
    );
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("slope", value);
    Ok(())
}

/*
Implements hint:
%{
    from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack

    slope = pack(ids.slope, PRIME)
    x = pack(ids.point.x, PRIME)
    y = pack(ids.point.y, PRIME)

    value = new_x = (pow(slope, 2, SECP_P) - 2 * x) % SECP_P
%}
*/
pub fn ec_double_assign_new_x(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //ids.slope
    let slope_reloc = get_relocatable_from_var_name("slope", ids, vm_proxy, hint_ap_tracking)?;

    let (slope_d0, slope_d1, slope_d2) = (
        vm_proxy.memory.borrow().get_integer(&slope_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&slope_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&slope_reloc + 2))?,
    );

    //ids.point
    let point_reloc = get_relocatable_from_var_name("point", ids, vm_proxy, hint_ap_tracking)?;

    let (x_d0, x_d1, x_d2, y_d0, y_d1, y_d2) = (
        vm_proxy.memory.borrow().get_integer(&point_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 2))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 3))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 4))?,
        vm_proxy.memory.borrow().get_integer(&(&point_reloc + 5))?,
    );

    let slope = pack(slope_d0, slope_d1, slope_d2, vm_proxy.prime);
    let x = pack(x_d0, x_d1, x_d2, vm_proxy.prime);
    let y = pack(y_d0, y_d1, y_d2, vm_proxy.prime);

    let value = (slope.pow(2) - (&x << 1_usize)).mod_floor(&SECP_P);

    //Assign variables to vm scope
    exec_scopes_proxy.insert_value("slope", slope);
    exec_scopes_proxy.insert_value("x", x);
    exec_scopes_proxy.insert_value("y", y);
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("new_x", value);
    Ok(())
}

/*
Implements hint:
%{ value = new_y = (slope * (x - new_x) - y) % SECP_P %}
*/
pub fn ec_double_assign_new_y(
    exec_scopes_proxy: &mut ExecutionScopesProxy,
) -> Result<(), VirtualMachineError> {
    //Get variables from vm scope
    let (slope, x, new_x, y) = (
        exec_scopes_proxy.get_int("slope")?,
        exec_scopes_proxy.get_int("x")?,
        exec_scopes_proxy.get_int("new_x")?,
        exec_scopes_proxy.get_int("y")?,
    );

    let value = (slope * (x - new_x) - y).mod_floor(&SECP_P);
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("new_y", value);
    Ok(())
}

/*
Implements hint:
%{
    from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack

    slope = pack(ids.slope, PRIME)
    x0 = pack(ids.point0.x, PRIME)
    x1 = pack(ids.point1.x, PRIME)
    y0 = pack(ids.point0.y, PRIME)

    value = new_x = (pow(slope, 2, SECP_P) - x0 - x1) % SECP_P
%}
*/
pub fn fast_ec_add_assign_new_x(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //ids.slope
    let slope_reloc = get_relocatable_from_var_name("slope", ids, vm_proxy, hint_ap_tracking)?;

    let (slope_d0, slope_d1, slope_d2) = (
        vm_proxy.memory.borrow().get_integer(&slope_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&slope_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&slope_reloc + 2))?,
    );

    //ids.point0
    let point0_reloc = get_relocatable_from_var_name("point0", ids, vm_proxy, hint_ap_tracking)?;

    let (point0_x_d0, point0_x_d1, point0_x_d2, point0_y_d0, point0_y_d1, point0_y_d2) = (
        vm_proxy.memory.borrow().get_integer(&point0_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 2))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 3))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 4))?,
        vm_proxy.memory.borrow().get_integer(&(&point0_reloc + 5))?,
    );

    //ids.point1.x
    let point1_reloc = get_relocatable_from_var_name("point1", ids, vm_proxy, hint_ap_tracking)?;

    let (point1_x_d0, point1_x_d1, point1_x_d2) = (
        vm_proxy.memory.borrow().get_integer(&point1_reloc)?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 1))?,
        vm_proxy.memory.borrow().get_integer(&(&point1_reloc + 2))?,
    );

    let slope = pack(slope_d0, slope_d1, slope_d2, vm_proxy.prime);
    let x0 = pack(point0_x_d0, point0_x_d1, point0_x_d2, vm_proxy.prime);
    let x1 = pack(point1_x_d0, point1_x_d1, point1_x_d2, vm_proxy.prime);
    let y0 = pack(point0_y_d0, point0_y_d1, point0_y_d2, vm_proxy.prime);

    let value = (slope.modpow(&bigint!(2), &SECP_P) - &x0 - x1).mod_floor(&SECP_P);

    //Assign variables to vm scope
    exec_scopes_proxy.insert_value("slope", slope);
    exec_scopes_proxy.insert_value("x0", x0);
    exec_scopes_proxy.insert_value("y0", y0);
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("new_x", value);

    Ok(())
}

/*
Implements hint:
%{ value = new_y = (slope * (x0 - new_x) - y0) % SECP_P %}
*/
pub fn fast_ec_add_assign_new_y(
    exec_scopes_proxy: &mut ExecutionScopesProxy,
) -> Result<(), VirtualMachineError> {
    //Get variables from vm scope
    let (slope, x0, new_x, y0) = (
        exec_scopes_proxy.get_int("slope")?,
        exec_scopes_proxy.get_int("x0")?,
        exec_scopes_proxy.get_int("new_x")?,
        exec_scopes_proxy.get_int("y0")?,
    );
    let value = (slope * (x0 - new_x) - y0).mod_floor(&SECP_P);
    exec_scopes_proxy.insert_value("value", value.clone());
    exec_scopes_proxy.insert_value("new_y", value);

    Ok(())
}

/*
Implements hint:
%{ memory[ap] = (ids.scalar % PRIME) % 2 %}
*/
pub fn ec_mul_inner(
    vm_proxy: &mut VMProxy,
    ids: &HashMap<String, usize>,
    hint_ap_tracking: Option<&ApTracking>,
) -> Result<(), VirtualMachineError> {
    //(ids.scalar % PRIME) % 2
    let scalar = get_integer_from_var_name("scalar", ids, vm_proxy, hint_ap_tracking)?
        .mod_floor(vm_proxy.prime)
        .bitand(bigint!(1));
    insert_value_into_ap(vm_proxy.memory, vm_proxy.run_context, scalar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::exec_scope::{get_exec_scopes_proxy, ExecutionScopes};
    use crate::types::relocatable::MaybeRelocatable;
    use crate::utils::test_utils::*;
    use crate::vm::errors::memory_errors::MemoryError;
    use crate::vm::hints::execute_hint::{get_vm_proxy, BuiltinHintExecutor, HintReference};
    use crate::vm::runners::builtin_runner::RangeCheckBuiltinRunner;
    use crate::vm::vm_core::VirtualMachine;
    use crate::vm::vm_memory::memory::Memory;
    use crate::{any_box, bigint_str};
    use num_bigint::{BigInt, Sign};
    use std::any::Any;

    static HINT_EXECUTOR: BuiltinHintExecutor = BuiltinHintExecutor {};
    use crate::types::hint_executor::HintExecutor;

    #[test]
    fn run_ec_negate_ok() {
        let hint_code = "from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack\n\ny = pack(ids.point.y, PRIME) % SECP_P\n# The modulo operation in python always returns a nonnegative number.\nvalue = (-y) % SECP_P";
        let mut vm = vm_with_range_check!();

        vm.memory = memory![((1, 3), 2645i32), ((1, 4), 454i32), ((1, 5), 206i32)];
        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 1));
        //Create ids
        let ids = ids!["point"];
        //Create references
        vm.references = references!(1);
        let mut exec_scopes = ExecutionScopes::new();
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                hint_code,
                &ids,
                &ApTracking::new()
            ),
            Ok(())
        );
        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"115792089237316195423569751828682367333329274433232027476421668138471189901786"
            ))
        );
    }

    #[test]
    fn run_compute_doubling_slope_ok() {
        let hint_code = "from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack\nfrom starkware.python.math_utils import ec_double_slope\n\n# Compute the slope.\nx = pack(ids.point.x, PRIME)\ny = pack(ids.point.y, PRIME)\nvalue = slope = ec_double_slope(point=(x, y), alpha=0, p=SECP_P)";
        let mut vm = vm_with_range_check!();
        vm.memory = memory![
            ((1, 0), 614323u64),
            ((1, 1), 5456867u64),
            ((1, 2), 101208u64),
            ((1, 3), 773712524u64),
            ((1, 4), 77371252u64),
            ((1, 5), 5298795u64)
        ];

        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 1));

        //Create ids
        let ids = ids!["point"];

        //Create references
        vm.references = references!(1);

        let mut exec_scopes = ExecutionScopes::new();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                hint_code,
                &ids,
                &ApTracking::new()
            ),
            Ok(())
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"40442433062102151071094722250325492738932110061897694430475034100717288403728"
            ))
        );

        //Check 'slope' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("slope"),
            Ok(bigint_str!(
                b"40442433062102151071094722250325492738932110061897694430475034100717288403728"
            ))
        );
    }

    #[test]
    fn run_compute_slope_ok() {
        let hint_code = "from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack\nfrom starkware.python.math_utils import line_slope\n\n# Compute the slope.\nx0 = pack(ids.point0.x, PRIME)\ny0 = pack(ids.point0.y, PRIME)\nx1 = pack(ids.point1.x, PRIME)\ny1 = pack(ids.point1.y, PRIME)\nvalue = slope = line_slope(point1=(x0, y0), point2=(x1, y1), p=SECP_P)";
        let mut vm = vm_with_range_check!();

        //Insert ids.point0 and ids.point1 into memory
        vm.memory = memory![
            ((1, 0), 134),
            ((1, 1), 5123),
            ((1, 2), 140),
            ((1, 3), 1232),
            ((1, 4), 4652),
            ((1, 5), 720),
            ((1, 6), 156),
            ((1, 7), 6545),
            ((1, 8), 100010),
            ((1, 9), 1123),
            ((1, 10), 1325),
            ((1, 11), 910)
        ];

        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 14));

        //Create ids
        let ids = ids!["point0", "point1"];

        //Create references
        vm.references = not_continuous_references![-14, -8];

        let mut exec_scopes = ExecutionScopes::new();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                &hint_code,
                &ids,
                &ApTracking::new()
            ),
            Ok(())
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"41419765295989780131385135514529906223027172305400087935755859001910844026631"
            ))
        );

        //Check 'slope' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("slope"),
            Ok(bigint_str!(
                b"41419765295989780131385135514529906223027172305400087935755859001910844026631"
            ))
        );
    }

    #[test]
    fn run_ec_double_assign_new_x_ok() {
        let hint_code = "from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack\n\nslope = pack(ids.slope, PRIME)\nx = pack(ids.point.x, PRIME)\ny = pack(ids.point.y, PRIME)\n\nvalue = new_x = (pow(slope, 2, SECP_P) - 2 * x) % SECP_P";
        let mut vm = vm_with_range_check!();

        //Insert ids.point and ids.slope into memory
        vm.memory = memory![
            ((1, 0), 134),
            ((1, 1), 5123),
            ((1, 2), 140),
            ((1, 3), 1232),
            ((1, 4), 4652),
            ((1, 5), 720),
            ((1, 6), 44186171158942157784255469_i128),
            ((1, 7), 54173758974262696047492534_i128),
            ((1, 8), 8106299688661572814170174_i128)
        ];

        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 10));

        //Create ids
        let ids = ids!["point", "slope"];

        //Create references
        vm.references = not_continuous_references![-10, -4];

        let mut exec_scopes = ExecutionScopes::new();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                &hint_code,
                &ids,
                &ApTracking::default()
            ),
            Ok(())
        );

        //Check 'slope' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("slope"),
            Ok(bigint_str!(
                b"48526828616392201132917323266456307435009781900148206102108934970258721901549"
            ))
        );

        //Check 'x' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("x"),
            Ok(bigint_str!(
                b"838083498911032969414721426845751663479194726707495046"
            ))
        );

        //Check 'y' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("y"),
            Ok(bigint_str!(
                b"4310143708685312414132851373791311001152018708061750480"
            ))
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"59479631769792988345961122678598249997181612138456851058217178025444564264149"
            ))
        );

        //Check 'new_x' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("new_x"),
            Ok(bigint_str!(
                b"59479631769792988345961122678598249997181612138456851058217178025444564264149"
            ))
        );
    }

    #[test]
    fn run_ec_double_assign_new_y_ok() {
        let hint_code = "value = new_y = (slope * (x - new_x) - y) % SECP_P";
        let mut vm = vm_with_range_check!();
        let mut exec_scopes = ExecutionScopes::new();

        //Insert 'slope' into vm scope
        exec_scopes.assign_or_update_variable(
            "slope",
            any_box!(bigint_str!(
                b"48526828616392201132917323266456307435009781900148206102108934970258721901549"
            )),
        );

        //Insert 'x' into vm scope
        exec_scopes.assign_or_update_variable(
            "x",
            any_box!(bigint_str!(
                b"838083498911032969414721426845751663479194726707495046"
            )),
        );

        //Insert 'new_x' into vm scope
        exec_scopes.assign_or_update_variable(
            "new_x",
            any_box!(bigint_str!(
                b"59479631769792988345961122678598249997181612138456851058217178025444564264149"
            )),
        );

        //Insert 'y' into vm scope
        exec_scopes.assign_or_update_variable(
            "y",
            any_box!(bigint_str!(
                b"4310143708685312414132851373791311001152018708061750480"
            )),
        );
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                hint_code,
                &HashMap::<String, usize>::new(),
                &ApTracking::new()
            ),
            Ok(())
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"7948634220683381957329555864604318996476649323793038777651086572350147290350"
            ))
        );

        //Check 'new_y' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("new_y"),
            Ok(bigint_str!(
                b"7948634220683381957329555864604318996476649323793038777651086572350147290350"
            ))
        );
    }

    #[test]
    fn run_fast_ec_add_assign_new_x_ok() {
        let hint_code = "from starkware.cairo.common.cairo_secp.secp_utils import SECP_P, pack\n\nslope = pack(ids.slope, PRIME)\nx0 = pack(ids.point0.x, PRIME)\nx1 = pack(ids.point1.x, PRIME)\ny0 = pack(ids.point0.y, PRIME)\n\nvalue = new_x = (pow(slope, 2, SECP_P) - x0 - x1) % SECP_P";
        let mut vm = vm_with_range_check!();

        //Insert ids.point0, ids.point1.x and ids.slope into memory
        vm.memory = memory![
            //ids.point0
            ((1, 0), 89712),
            ((1, 1), 56),
            ((1, 2), 1233409),
            ((1, 3), 980126),
            ((1, 4), 10),
            ((1, 5), 8793),
            //ids.point0.x
            ((1, 6), 1235216451),
            ((1, 7), 5967),
            ((1, 8), 2171381),
            //ids.slope
            ((1, 9), 67470097831679799377177424_i128),
            ((1, 10), 43370026683122492246392730_i128),
            ((1, 11), 16032182557092050689870202_i128)
        ];

        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 15));

        //Initialize ap
        vm.run_context.ap = MaybeRelocatable::from((1, 20));

        //Create ids
        let ids = ids!["point0", "point1", "slope"];

        //Create references
        vm.references = not_continuous_references![-15, -9, -6];

        let mut exec_scopes = ExecutionScopes::new();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                &hint_code,
                &ids,
                &ApTracking::default()
            ),
            Ok(())
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"8891838197222656627233627110766426698842623939023296165598688719819499152657"
            ))
        );

        //Check 'new_x' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("new_x"),
            Ok(bigint_str!(
                b"8891838197222656627233627110766426698842623939023296165598688719819499152657"
            ))
        );
    }

    #[test]
    fn run_fast_ec_add_assign_new_y_ok() {
        let hint_code = "value = new_y = (slope * (x0 - new_x) - y0) % SECP_P";
        let mut vm = vm_with_range_check!();
        let mut exec_scopes = ExecutionScopes::new();
        //Insert 'slope' into vm scope
        exec_scopes.assign_or_update_variable(
            "slope",
            any_box!(bigint_str!(
                b"48526828616392201132917323266456307435009781900148206102108934970258721901549"
            )),
        );

        //Insert 'x0' into vm scope
        exec_scopes.assign_or_update_variable(
            "x0",
            any_box!(bigint_str!(
                b"838083498911032969414721426845751663479194726707495046"
            )),
        );

        //Insert 'new_x' into vm scope
        exec_scopes.assign_or_update_variable(
            "new_x",
            any_box!(bigint_str!(
                b"59479631769792988345961122678598249997181612138456851058217178025444564264149"
            )),
        );

        //Insert 'y0' into vm scope
        exec_scopes.assign_or_update_variable(
            "y0",
            any_box!(bigint_str!(
                b"4310143708685312414132851373791311001152018708061750480"
            )),
        );
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let exec_scopes_proxy = &mut get_exec_scopes_proxy(&mut exec_scopes);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy,
                hint_code,
                &HashMap::<String, usize>::new(),
                &ApTracking::new()
            ),
            Ok(())
        );

        //Check 'value' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("value"),
            Ok(bigint_str!(
                b"7948634220683381957329555864604318996476649323793038777651086572350147290350"
            ))
        );

        //Check 'new_y' is defined in the vm scope
        assert_eq!(
            exec_scopes_proxy.get_int("new_y"),
            Ok(bigint_str!(
                b"7948634220683381957329555864604318996476649323793038777651086572350147290350"
            ))
        );
    }

    #[test]
    fn run_ec_mul_inner_ok() {
        let hint_code = "memory[ap] = (ids.scalar % PRIME) % 2";
        let mut vm = vm_with_range_check!();

        let scalar = 89712_i32 + &vm.prime;
        //Insert ids.scalar into memory
        vm.memory = memory![((1, 0), scalar)];

        //Initialize fp
        vm.run_context.fp = MaybeRelocatable::from((1, 1));

        //Initialize ap
        vm.run_context.ap = MaybeRelocatable::from((1, 2));

        //Create ids
        let ids = ids!["scalar"];

        //Create references
        vm.references = references!(1);

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
assert_eq!(
            hint_processor.execute_hint(
                vm_proxy,
                exec_scopes_proxy_ref!(),
                &hint_code,
                &ids,
                &ApTracking::new()
            ),
            Ok(())
        );

        //Check hint memory inserts
        check_memory![&vm.memory, ((1, 2), 0)];
    }
}
