// Actual existing raw-touch fixture and ordinary MenuInput consumer. These flags
// are presentation inputs on the real model, not claims of original mod execution.
#[test]
fn actual_touch_barter_honors_main_full_total_and_caps_after_without_changing_payment() {
    for native in [false, true] {
        let (mut app, stock)=scene(native,crate::PlayState::Menu);
        app.world_mut().resource_mut::<crate::sim::SimState>().world.inventory_mut(stock).unwrap().remove(CAPS,999);
        let token=row(&app,Side::Player,GOODS);
        tap(&mut app,Target::Row(token));tap(&mut app,Target::All);
        assert_eq!(offered(&app),8);
        let trade=model(&app).trade.as_ref().unwrap();
        assert_eq!(trade.balance,72);assert_eq!(trade.paid(),1);
        let basic="Your caps 1000 · Merchant caps 1 · Receive 1 — merchant caps limit · Basket buy 0 / sell 8";
        let mut texts=app.world_mut().query::<&Text>();
        assert!(texts.iter(app.world()).any(|text|text.0==basic),"default keeps the original capped figure");
        {
            let mut active=app.world_mut().resource_mut::<ActiveMenu>();let menu=active.0.as_mut().unwrap();
            menu.full_barter_total=true;menu.barter_caps_change=true;
        }
        app.update();
        let mut texts=app.world_mut().query::<&Text>();
        assert!(texts.iter(app.world()).any(|text|text.0=="Your Caps  1000 (to 1001) · Merchant Caps  1 (to 0) · Receive 72 — merchant caps limit · Basket buy 0 / sell 8"));
        assert_eq!((count(&mut app,PLAYER,CAPS),count(&mut app,stock,CAPS)),(1000,1),"changing figures cannot transfer caps");
        assert_eq!((count(&mut app,PLAYER,GOODS),count(&mut app,stock,GOODS)),(8,8));
        tap(&mut app,Target::Accept);
        assert_eq!((count(&mut app,PLAYER,CAPS),count(&mut app,stock,CAPS)),(1001,0),"actual existing consumer still pays the merchant-capped amount");
        assert_eq!((count(&mut app,PLAYER,GOODS),count(&mut app,stock,GOODS)),(0,16));
        assert_eq!(offered(&app),0);
    }
}
